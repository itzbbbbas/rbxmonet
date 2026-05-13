use log::info;

use crate::Result;
use crate::api::products::fetch_all_products;
use crate::sync::products::{MultiProduct, Product, ProductType, SubscriptionEntry, VCSProducts};
use crate::utils::{canonical_name, format_name, is_censored};

pub struct Downloader {
    local_products: VCSProducts,
    remote_products: Vec<MultiProduct>,
}

impl Downloader {
    async fn create() -> Result<Self> {
        info!("fetching local products");
        let local_products_data = VCSProducts::get_products().await?;

        info!("fetching remote products");
        let remote_product_data =
            fetch_all_products(local_products_data.metadata.universe_id).await?;

        info!(
            "fetched {} local products, {} remote products",
            local_products_data.gamepasses.len()
                + local_products_data.products.len()
                + local_products_data.subscriptions.len()
                + local_products_data.badges.len(),
            remote_product_data.len()
        );

        Ok(Downloader {
            local_products: local_products_data,
            remote_products: remote_product_data,
        })
    }

    pub async fn download(overwrite: bool) -> Result<()> {
        let downloader = Downloader::create().await?;

        let mut local_products_data = downloader.local_products;
        let remote_product_data = downloader.remote_products;

        let filters = &local_products_data.metadata.name_filters;

        info!(
            "merging local products, and remote products (overwrite: {})",
            overwrite
        );

        remote_product_data.iter().for_each(|multi_product| {
            // Subscriptions have string ids and a slimmer schema; handle separately.
            if let MultiProduct::Subscription(sub) = multi_product {
                let name = format_name(canonical_name(sub.name.clone(), &filters));

                let existing_key = local_products_data
                    .subscriptions
                    .iter()
                    .find(|(_, x)| x.id == sub.id)
                    .map(|(k, _)| k.clone());

                let key = existing_key.clone().unwrap_or_else(|| name.clone());
                let existing_entry = existing_key
                    .as_ref()
                    .and_then(|k| local_products_data.subscriptions.get(k).cloned());

                let merged = SubscriptionEntry {
                    id: sub.id.clone(),
                    name: if !overwrite
                        && let Some(existing) = &existing_entry
                    {
                        existing.name.clone()
                    } else {
                        sub.name.clone()
                    },
                    description: if !overwrite
                        && let Some(existing) = &existing_entry
                    {
                        existing.description.clone()
                    } else {
                        sub.description.clone()
                    },
                    active: sub.active,
                    price: if let Some(existing) = &existing_entry {
                        if overwrite { sub.price } else { existing.price }
                    } else {
                        sub.price
                    },
                };

                local_products_data.subscriptions.insert(key, merged);
                return;
            }

            let (product, product_type): (Product, ProductType) = match multi_product {
                MultiProduct::GamePass(prod) => (prod.clone(), ProductType::GamePass),
                MultiProduct::DevProduct(prod) => (prod.clone(), ProductType::DevProduct),
                MultiProduct::Badge(prod) => (prod.clone(), ProductType::Badge),
                MultiProduct::Subscription(_) => unreachable!(),
            };

            let name = format_name(canonical_name(product.name.clone(), &filters));

            let existing = match product_type {
                ProductType::GamePass => local_products_data.gamepasses.iter().find(|(_, x)| {
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

                ProductType::Subscription => unreachable!(),
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
                true => name.clone(),
                false => existing.unwrap().0.clone(),
            };

            match product_type {
                ProductType::GamePass => local_products_data.gamepasses.insert(key, product),
                ProductType::DevProduct => local_products_data.products.insert(key, product),
                ProductType::Badge => local_products_data.badges.insert(key, product),
                ProductType::Subscription => unreachable!(),
            };
        });

        info!("finished merging products, saving to disk");
        local_products_data.save_products().await?;

        info!("serializing products to luau format");
        local_products_data.serialize_luau().await?;

        Ok(())
    }
}
