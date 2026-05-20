use std::collections::HashMap;

use log::info;

use crate::Result;
use crate::api::model::{BadgeUpdateRequest, ProductUpdateRequest};
use crate::api::products::{
    create_badge, create_dev_product, create_pass, fetch_all_products, fetch_badge_metadata,
    fetch_free_badges_quota, hash_icon_file, update_badge, update_badge_icon, update_dev_product,
    update_pass,
};

const DEFAULT_ICON: &str = "assets/Image/missing_texture.png";
use crate::sync::products::{MultiProduct, Product, ProductType, VCSProducts};
use crate::ui::confirm::{ConfirmState, ConfirmViewer};
use crate::ui::diffs::{DiffChange, DiffViewer, ProductDiff, ProductDiffs};

#[derive(Clone)]
struct IconDirty {
    path: String,
    new_hash: String,
    old_hash: Option<String>,
}

fn short_hash(h: &str) -> String {
    h.chars().take(8).collect()
}

pub struct Uploader {
    local_products: VCSProducts,
    remote_products: Vec<MultiProduct>,
}

fn apply_discount_prefix(product: &mut Product, prefix: Option<String>) {
    let prefix = prefix.unwrap_or("💲{}% OFF💲".to_string());

    if product.has_discount() {
        product.name = format!(
            "{} {}",
            prefix.replace("{}", &product.discount.unwrap_or(0).to_string()),
            product.name
        );
    }
}

impl Uploader {
    fn has_empty_products(&self) -> bool {
        let has_empty_passes = self
            .local_products
            .passes
            .iter()
            .any(|(_, gp)| gp.id.is_none());
        let has_empty_devproducts = self
            .local_products
            .products
            .iter()
            .any(|(_, dp)| dp.id.is_none());

        has_empty_passes || has_empty_devproducts
    }

    async fn upload_badge_creates(&mut self) -> Result<()> {
        let universe_id = self.local_products.metadata.universe_id;

        let slugs: Vec<String> = self
            .local_products
            .badges
            .iter()
            .filter(|(_, b)| b.id.is_none())
            .map(|(k, _)| k.clone())
            .collect();

        if slugs.is_empty() {
            return Ok(());
        }

        let mut metadata_cost: Option<u64> = None;
        let mut free_quota: Option<u64> = match fetch_free_badges_quota(universe_id).await {
            Ok(q) => Some(q),
            Err(e) => {
                log::warn!("failed to fetch free-badges-quota: {}", e);
                None
            }
        };
        if let Some(q) = free_quota {
            info!("free badge quota: {}", q);
        }

        for slug in slugs {
            let badge = match self.local_products.badges.get(&slug) {
                Some(b) => b.clone(),
                None => continue,
            };

            let icon_path = badge
                .icon
                .clone()
                .unwrap_or_else(|| DEFAULT_ICON.to_string());

            let expected_cost = if free_quota.unwrap_or(0) > 0 {
                0
            } else {
                if metadata_cost.is_none() {
                    match fetch_badge_metadata().await {
                        Ok(md) => metadata_cost = Some(md.badge_creation_price),
                        Err(e) => {
                            log::error!("failed to fetch badge metadata: {}", e);
                            return Ok(());
                        }
                    }
                }
                metadata_cost.unwrap_or(100)
            };

            let desc = badge.description.clone().unwrap_or_default();
            match create_badge(
                universe_id,
                &badge.name,
                &desc,
                badge.active,
                &icon_path,
                expected_cost,
            )
            .await
            {
                Ok(new) => {
                    info!(
                        "created Badge '{}' (id: {}, cost: {} R$)",
                        badge.name, new.id, expected_cost
                    );
                    let new_hash = hash_icon_file(&icon_path).await;
                    if let Some(b) = self.local_products.badges.get_mut(&slug) {
                        b.id = Some(new.id);
                        if let Some(icon_id) = new.icon_image_id {
                            b.icon_id = Some(icon_id);
                        }
                        if let Some(h) = new_hash {
                            b.icon_hash = Some(h);
                        }
                    }
                    if let Some(q) = free_quota.as_mut()
                        && *q > 0
                    {
                        *q -= 1;
                    }
                }
                Err(e) => log::error!("failed to create badge '{}': {}", slug, e),
            }
        }

        Ok(())
    }

    async fn upload_empty(&mut self, overwrite: bool) -> Result<()> {
        self.upload_badge_creates().await?;

        if !self.has_empty_products() {
            return Ok(());
        }

        if !overwrite {
            let prompt_result =
                ConfirmViewer::show_prompt("Would you like to upload non-existant products?").await;

            if prompt_result != ConfirmState::Confirmed {
                info!("not uploading non-existant products");
                return Ok(());
            }
        }

        let universe_id = self.local_products.metadata.universe_id.clone();
        let upload_product = async |universe_id: u64,
                                    product: Product,
                                    product_type: ProductType|
              -> Result<(u64, Option<u64>)> {
            let update_request = ProductUpdateRequest::from(&product);
            let icon_path = product.icon.as_deref();
            let (product_id, icon_id) = match product_type {
                ProductType::Pass => {
                    let gp = create_pass(universe_id, &update_request, icon_path).await?;
                    (gp.game_pass_id, Some(gp.icon_asset_id))
                }

                ProductType::DevProduct => {
                    let dp = create_dev_product(universe_id, &update_request, icon_path).await?;
                    (dp.product_id, dp.icon_image_asset_id)
                }

                ProductType::Badge => {
                    return Err("badge creates handled by upload_badge_creates".into());
                }
            };

            info!(
                "uploaded {:?} '{}' with id {}",
                product_type, product.name, product_id
            );

            Ok((product_id, icon_id))
        };

        let mut pass_futures = vec![];
        let mut devproduct_futures = vec![];

        info!(
            "uploading {} products, and {} passes in universe {}",
            self.local_products.products.len(),
            self.local_products.passes.len(),
            universe_id
        );

        for (name, pass) in &self.local_products.passes {
            if pass.id.is_none() {
                let universe_id = universe_id.clone();
                let name = name.clone();
                let icon_path = pass.icon.clone();
                let mut pass = pass.clone();

                apply_discount_prefix(
                    &mut pass,
                    self.local_products.metadata.discount_prefix.clone(),
                );

                let future = (async move {
                    let result = upload_product(universe_id, pass, ProductType::Pass).await;

                    match result {
                        Ok((id, icon_id)) => {
                            let new_hash = match &icon_path {
                                Some(p) => hash_icon_file(p).await,
                                None => None,
                            };
                            Some((name, id, icon_id, new_hash))
                        }
                        Err(e) => {
                            log::error!("failed to upload pass '{}': {}", name, e);
                            None
                        }
                    }
                })
                .await;

                pass_futures.push(future);
            }
        }

        for (name, devproduct) in &self.local_products.products {
            if devproduct.id.is_none() {
                let universe_id = universe_id.clone();
                let name = name.clone();
                let icon_path = devproduct.icon.clone();
                let mut devproduct = devproduct.clone();

                apply_discount_prefix(
                    &mut devproduct,
                    self.local_products.metadata.discount_prefix.clone(),
                );

                let future = (async move {
                    let result =
                        upload_product(universe_id, devproduct.clone(), ProductType::DevProduct)
                            .await;

                    match result {
                        Ok((id, icon_id)) => {
                            let new_hash = match &icon_path {
                                Some(p) => hash_icon_file(p).await,
                                None => None,
                            };
                            Some((name, id, icon_id, new_hash))
                        }
                        Err(e) => {
                            log::error!("failed to upload dev product '{}': {}", name, e);
                            None
                        }
                    }
                })
                .await;

                devproduct_futures.push(future);
            }
        }

        pass_futures.into_iter().for_each(|res| {
            if let Some((name, id, icon_id, icon_hash)) = res {
                let p = self.local_products.passes.get_mut(name.as_str()).unwrap();
                p.id = Some(id as u64);
                if let Some(icon_id) = icon_id {
                    p.icon_id = Some(icon_id);
                }
                if let Some(h) = icon_hash {
                    p.icon_hash = Some(h);
                }
            }
        });

        devproduct_futures.into_iter().for_each(|res| {
            if let Some((name, id, icon_id, icon_hash)) = res {
                let p = self.local_products.products.get_mut(name.as_str()).unwrap();
                p.id = Some(id as u64);
                if let Some(icon_id) = icon_id {
                    p.icon_id = Some(icon_id);
                }
                if let Some(h) = icon_hash {
                    p.icon_hash = Some(h);
                }
            }
        });

        self.local_products.save_products().await?;
        self.local_products.save_lock().await?;
        self.local_products.serialize_luau().await?;

        Ok(())
    }

    async fn upload_modified(
        &mut self,
        overwrite: bool,
        auto_confirm: bool,
        force_icons: bool,
    ) -> Result<()> {
        let mut product_diffs = vec![];

        let universe_id = self.local_products.metadata.universe_id.clone();
        let products = &self.remote_products;
        let mut all_local_products = vec![];

        all_local_products.extend(self.local_products.passes.values().cloned());
        all_local_products.extend(self.local_products.products.values().cloned());
        all_local_products.extend(self.local_products.badges.values().cloned());

        // Pre-pass: hash every local icon, compare to stored icon_hash. Build
        // map keyed by (product_type, id) so the diff phase can inject a
        // ProductDiff::Icon row and so the push phase knows whether to send
        // the multipart icon part.
        let mut icon_dirty: HashMap<(ProductType, u64), IconDirty> = HashMap::new();
        for local_product in &all_local_products {
            let Some(id) = local_product.id else { continue };
            let Some(icon_path) = local_product.icon.as_deref() else {
                continue;
            };
            let Some(new_hash) = hash_icon_file(icon_path).await else {
                continue;
            };
            let old_hash = local_product.icon_hash.clone();
            let needs = force_icons || old_hash.as_deref() != Some(new_hash.as_str());
            if !needs {
                continue;
            }
            let product_type = if self
                .local_products
                .passes
                .values()
                .any(|p| p.id == Some(id))
            {
                ProductType::Pass
            } else if self
                .local_products
                .products
                .values()
                .any(|p| p.id == Some(id))
            {
                ProductType::DevProduct
            } else if self
                .local_products
                .badges
                .values()
                .any(|p| p.id == Some(id))
            {
                ProductType::Badge
            } else {
                continue;
            };
            icon_dirty.insert(
                (product_type, id),
                IconDirty {
                    path: icon_path.to_string(),
                    new_hash,
                    old_hash,
                },
            );
        }

        product_diffs.extend(
            all_local_products
                .iter()
                .filter_map(|local_product| {
                    let id = match local_product.id {
                        Some(id) => id,
                        None => return None,
                    };

                    let (product_type, remote_product) =
                        match products.iter().find(|multi_product| match multi_product {
                            MultiProduct::Pass(pass) => pass.id.unwrap() == id,
                            MultiProduct::DevProduct(prod) => prod.id.unwrap() == id,
                            MultiProduct::Badge(b) => b.id.unwrap() == id,
                        }) {
                            Some(MultiProduct::Pass(pass)) => (ProductType::Pass, pass),
                            Some(MultiProduct::DevProduct(prod)) => (ProductType::DevProduct, prod),
                            Some(MultiProduct::Badge(badge)) => (ProductType::Badge, badge),
                            None => return None,
                        };

                    let computed = local_product
                        .diff(&remote_product, Some(&self.local_products.metadata));

                    let icon_row =
                        icon_dirty.get(&(product_type, id)).map(|dirty| {
                            DiffChange::Changed(ProductDiff::Icon(
                                dirty
                                    .old_hash
                                    .as_deref()
                                    .map(short_hash)
                                    .unwrap_or_else(|| "<unset>".to_string()),
                                short_hash(&dirty.new_hash),
                            ))
                        });

                    match (computed, icon_row) {
                        (Some(mut diff), Some(row)) => {
                            diff.diffs.push(row);
                            Some((product_type, diff))
                        }
                        (Some(diff), None) => Some((product_type, diff)),
                        (None, Some(row)) => Some((
                            product_type,
                            ProductDiffs {
                                name: local_product.name.clone(),
                                id,
                                diffs: vec![row],
                            },
                        )),
                        (None, None) => None,
                    }
                })
                .collect::<Vec<_>>(),
        );

        let mut all_diffs = vec![] as Vec<(ProductType, ProductDiffs)>;
        all_diffs.extend(product_diffs);
        all_diffs.sort_by(|a, b| match b.0.cmp(&a.0) {
            std::cmp::Ordering::Equal => a.1.id.cmp(&b.1.id),
            other => other,
        });

        if all_diffs.len() == 0 {
            info!("no differences found between local and universe products.");
            return Ok(());
        }

        let diffs: Vec<(ProductType, u64)>;

        if overwrite || auto_confirm {
            diffs = all_diffs
                .iter()
                .cloned()
                .map(|(product_type, diff)| (product_type, diff.id))
                .collect::<Vec<_>>();
            if auto_confirm && !overwrite {
                info!(
                    "auto-confirm: applying {} diff(s) without prompt",
                    diffs.len()
                );
            }
        } else {
            diffs = DiffViewer::confirm_diffs(all_diffs.iter().cloned().collect()).await;

            let apply = ConfirmViewer::show_prompt("Would you like to sync products?").await;

            if apply == ConfirmState::Closed {
                info!("user aborted sync.");
                return Ok(());
            }
        }

        if diffs.len() == 0 {
            info!("No changes to apply.");
            return Ok(());
        }

        let total = diffs.len();
        info!("syncing {} product(s)", total);

        let mut succeeded = 0usize;
        let mut failed: Vec<(ProductType, u64, String, String)> = Vec::new();

        for (product_type, id) in diffs {
            let mut local_product = match product_type {
                ProductType::Pass => self
                    .local_products
                    .passes
                    .values()
                    .find(|gp| gp.id == Some(id)),
                ProductType::DevProduct => self
                    .local_products
                    .products
                    .values()
                    .find(|prod| prod.id == Some(id)),
                ProductType::Badge => self
                    .local_products
                    .badges
                    .values()
                    .find(|b| b.id == Some(id)),
            }
            .unwrap()
            .clone();

            let name = local_product.name.clone();

            apply_discount_prefix(
                &mut local_product,
                self.local_products.metadata.discount_prefix.clone(),
            );

            let update_request = ProductUpdateRequest::from(&local_product);
            let dirty = icon_dirty.get(&(product_type, id)).cloned();
            let icon_path = dirty.as_ref().map(|d| d.path.as_str());

            let result: Result<Option<u64>> = match product_type {
                ProductType::Pass => {
                    update_pass(universe_id, id, &update_request, icon_path).await
                }
                ProductType::DevProduct => {
                    update_dev_product(universe_id, id, &update_request, icon_path).await
                }
                ProductType::Badge => {
                    let badge_request = BadgeUpdateRequest::from(&local_product);
                    let metadata_result = update_badge(id, &badge_request).await;
                    if let Err(e) = metadata_result {
                        Err(e)
                    } else if let Some(d) = &dirty {
                        update_badge_icon(id, &d.path).await
                    } else {
                        Ok(None)
                    }
                }
            };

            match result {
                Ok(new_icon_id) => {
                    succeeded += 1;
                    info!("synced {:?} '{}' (id: {})", product_type, name, id);

                    let local_mut: Option<&mut Product> = match product_type {
                        ProductType::Pass => self
                            .local_products
                            .passes
                            .values_mut()
                            .find(|gp| gp.id == Some(id)),
                        ProductType::DevProduct => self
                            .local_products
                            .products
                            .values_mut()
                            .find(|p| p.id == Some(id)),
                        ProductType::Badge => self
                            .local_products
                            .badges
                            .values_mut()
                            .find(|b| b.id == Some(id)),
                    };
                    if let Some(p) = local_mut {
                        if let Some(new_id) = new_icon_id {
                            p.icon_id = Some(new_id);
                        }
                        if let Some(d) = &dirty {
                            p.icon_hash = Some(d.new_hash.clone());
                        }
                    }
                }
                Err(e) => {
                    let err_str = e.to_string();
                    log::error!(
                        "failed to sync {:?} '{}' (id: {}): {} \u{2014} continuing",
                        product_type,
                        name,
                        id,
                        err_str
                    );
                    failed.push((product_type, id, name, err_str));
                }
            }
        }

        if failed.is_empty() {
            info!("finished syncing all {} product(s)", total);
        } else {
            log::warn!(
                "sync finished: {}/{} ok, {} failed",
                succeeded,
                total,
                failed.len()
            );
            for (pt, id, name, err) in &failed {
                log::warn!("  failed {:?} '{}' (id: {}): {}", pt, name, id, err);
            }
        }

        Ok(())
    }

    async fn create() -> Result<Self> {
        info!("fetching local products");
        let local_products_data = VCSProducts::get_products().await?;

        info!("fetching remote products");
        let snapshot = fetch_all_products(local_products_data.metadata.universe_id).await?;

        info!(
            "fetched {} local products, {} remote products",
            local_products_data.passes.len()
                + local_products_data.products.len()
                + local_products_data.badges.len(),
            snapshot.products.len()
        );

        Ok(Self {
            local_products: local_products_data,
            remote_products: snapshot.products,
        })
    }

    pub async fn upload(overwrite: bool, auto_confirm: bool, force_icons: bool) -> Result<()> {
        let mut uploader = Uploader::create().await?;
        crate::alpha_bleed::set_bleed_enabled(uploader.local_products.icons.bleed);

        let mut run_upload = async || -> Result<()> {
            uploader.upload_empty(overwrite).await?;
            uploader
                .upload_modified(overwrite, auto_confirm, force_icons)
                .await?;

            Ok(())
        };

        let upload_result = run_upload().await;

        uploader.local_products.save_products().await?;
        uploader.local_products.save_lock().await?;
        uploader.local_products.serialize_luau().await?;

        if let Err(e) = upload_result {
            info!("failed to upload modified products: {}, aborting upload", e);
            return Err(e);
        }

        Ok(())
    }
}
