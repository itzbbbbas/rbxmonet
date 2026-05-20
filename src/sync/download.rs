use std::collections::HashSet;

use log::info;

use crate::Result;
use crate::api::products::{RemoteSnapshot, fetch_all_products};
use crate::sync::products::{MultiProduct, Product, ProductType, VCSProducts};
use crate::utils::{canonical_name, format_name, is_censored};

pub struct Downloader {
    local_products: VCSProducts,
    remote_snapshot: RemoteSnapshot,
}

impl Downloader {
    async fn create() -> Result<Self> {
        info!("fetching local products");
        let local_products_data = VCSProducts::get_products().await?;

        info!("fetching remote products");
        let remote_snapshot =
            fetch_all_products(local_products_data.metadata.universe_id).await?;

        info!(
            "fetched {} local products, {} remote products",
            local_products_data.passes.len()
                + local_products_data.products.len()
                + local_products_data.badges.len(),
            remote_snapshot.products.len()
        );

        Ok(Downloader {
            local_products: local_products_data,
            remote_snapshot,
        })
    }

    pub async fn download(overwrite: bool) -> Result<()> {
        let downloader = Downloader::create().await?;

        let mut local_products_data = downloader.local_products;
        let snapshot = downloader.remote_snapshot;
        let remote_product_data = &snapshot.products;

        let filters = &local_products_data.metadata.name_filters;

        info!(
            "merging local products, and remote products (overwrite: {})",
            overwrite
        );

        remote_product_data.iter().for_each(|multi_product| {
            let (product, product_type): (Product, ProductType) = match multi_product {
                MultiProduct::Pass(prod) => (prod.clone(), ProductType::Pass),
                MultiProduct::DevProduct(prod) => (prod.clone(), ProductType::DevProduct),
                MultiProduct::Badge(prod) => (prod.clone(), ProductType::Badge),
            };

            let name = format_name(canonical_name(product.name.clone(), &filters));

            let existing = match product_type {
                ProductType::Pass => local_products_data.passes.iter().find(|(_, x)| {
                    x.id.map(|id| id as i64).unwrap_or(-1)
                        == product.id.map(|id| id as i64).unwrap_or(-1)
                }),

                ProductType::DevProduct => local_products_data.products.iter().find(|(_, x)| {
                    x.id.map(|id| id as i64).unwrap_or(-1)
                        == product.id.map(|id| id as i64).unwrap_or(-1)
                }),

                ProductType::Badge => local_products_data.badges.iter().find(|(_, x)| {
                    x.id.map(|id| id as i64).unwrap_or(-1)
                        == product.id.map(|id| id as i64).unwrap_or(-1)
                }),
            };

            let mut product = Product {
                id: product.id,
                name: if !overwrite && existing.is_some() {
                    canonical_name(existing.unwrap().1.name.clone(), &filters)
                } else {
                    canonical_name(product.name.clone(), &filters)
                },
                prefix: if !overwrite && let Some(existing_product) = existing {
                    existing_product.1.prefix.clone()
                } else {
                    None
                },
                description: if !overwrite && let Some(existing_product) = existing {
                    existing_product.1.description.clone()
                } else {
                    product.description
                },
                active: product.active,
                discount: match existing {
                    Some((_, existing_product)) if existing_product.has_discount() => {
                        existing_product.discount
                    }
                    _ => None,
                },
                price: if let Some(existing_product) = existing {
                    if overwrite {
                        product.price
                    } else {
                        existing_product.1.price
                    }
                } else {
                    product.price
                },
                icon: existing.and_then(|(_, p)| p.icon.clone()),
                icon_id: product.icon_id.or_else(|| existing.and_then(|(_, p)| p.icon_id)),
                icon_hash: existing.and_then(|(_, p)| p.icon_hash.clone()),
                path: existing.and_then(|(_, p)| p.path.clone()),
                regional_pricing: if let Some(existing_product) = existing {
                    if overwrite {
                        product.regional_pricing
                    } else {
                        existing_product.1.regional_pricing
                    }
                } else {
                    product.regional_pricing
                },
            };

            if let Some(regional_pricing) = product.regional_pricing
                && !regional_pricing
            {
                product.regional_pricing = None;
            }

            if !overwrite && let Some(existing_product) = existing {
                if let Some(desc) = product.description.clone()
                    && is_censored(&desc)
                {
                    product.description = existing_product.1.description.clone();
                }
            }

            let key = match existing.is_none() {
                true => {
                    let section: &std::collections::HashMap<String, Product> = match product_type {
                        ProductType::Pass => &local_products_data.passes,
                        ProductType::DevProduct => &local_products_data.products,
                        ProductType::Badge => &local_products_data.badges,
                    };
                    disambiguate_slug(section, &name, product.id)
                }
                false => existing.unwrap().0.clone(),
            };

            match product_type {
                ProductType::Pass => local_products_data.passes.insert(key, product),
                ProductType::DevProduct => local_products_data.products.insert(key, product),
                ProductType::Badge => local_products_data.badges.insert(key, product),
            };
        });

        prune_stale(&mut local_products_data, &snapshot);

        info!("finished merging products, saving to disk");
        local_products_data.save_products().await?;
        local_products_data.save_lock().await?;

        info!("serializing products to luau format");
        local_products_data.serialize_luau().await?;

        Ok(())
    }
}

/// Pick a unique map key for a new remote product. If the canonical slug
/// is already taken by an entry with a *different* id, suffix with the
/// product id so the new entry doesn't silently overwrite. If the slug
/// is empty (canonical name reduced to nothing) fall back to `item_<id>`.
fn disambiguate_slug(
    section: &std::collections::HashMap<String, Product>,
    base: &str,
    incoming_id: Option<u64>,
) -> String {
    let base = if base.is_empty() {
        format!("item_{}", incoming_id.unwrap_or(0))
    } else {
        base.to_string()
    };

    match section.get(&base) {
        Some(existing) if existing.id != incoming_id => {
            let suffix = incoming_id.unwrap_or(0);
            log::warn!(
                "slug '{}' collides (existing id {:?} vs new id {:?}); using '{}_{}'",
                base,
                existing.id,
                incoming_id,
                base,
                suffix
            );
            format!("{}_{}", base, suffix)
        }
        _ => base,
    }
}

fn prune_stale(local: &mut VCSProducts, snapshot: &RemoteSnapshot) {
    let mut remote_pass_ids: HashSet<u64> = HashSet::new();
    let mut remote_devproduct_ids: HashSet<u64> = HashSet::new();
    let mut remote_badge_ids: HashSet<u64> = HashSet::new();

    for mp in &snapshot.products {
        match mp {
            MultiProduct::Pass(p) => {
                if let Some(id) = p.id {
                    remote_pass_ids.insert(id);
                }
            }
            MultiProduct::DevProduct(p) => {
                if let Some(id) = p.id {
                    remote_devproduct_ids.insert(id);
                }
            }
            MultiProduct::Badge(p) => {
                if let Some(id) = p.id {
                    remote_badge_ids.insert(id);
                }
            }
        }
    }

    if snapshot.passes_fetched {
        prune_section_u64(&mut local.passes, &remote_pass_ids, "pass");
    }
    if snapshot.dev_products_fetched {
        prune_section_u64(&mut local.products, &remote_devproduct_ids, "dev product");
    }
    if snapshot.badges_fetched {
        prune_section_u64(&mut local.badges, &remote_badge_ids, "badge");
    }
}

fn prune_section_u64(
    section: &mut std::collections::HashMap<String, Product>,
    remote_ids: &HashSet<u64>,
    label: &str,
) {
    let to_drop: Vec<String> = section
        .iter()
        .filter(|(_, v)| match v.id {
            Some(id) => !remote_ids.contains(&id),
            None => false,
        })
        .map(|(k, _)| k.clone())
        .collect();
    if !to_drop.is_empty() {
        info!(
            "pruned {} {} entr{} no longer in remote: {} (use `git checkout rbxmonet.toml` to undo)",
            to_drop.len(),
            label,
            if to_drop.len() == 1 { "y" } else { "ies" },
            to_drop.join(", "),
        );
        for k in &to_drop {
            section.remove(k);
        }
    }
}
