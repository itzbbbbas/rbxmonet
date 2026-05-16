use log::info;

use crate::Result;
use crate::api::model::{BadgeUpdateRequest, ProductUpdateRequest};
use crate::api::products::{
    create_badge, create_dev_product, create_pass, fetch_all_products, fetch_badge_metadata,
    fetch_free_badges_quota, update_badge, update_dev_product, update_pass,
};

const DEFAULT_ICON: &str = "assets/Image/missing_texture.png";
use crate::sync::products::{MultiProduct, Product, ProductType, VCSProducts};
use crate::ui::confirm::{ConfirmState, ConfirmViewer};
use crate::ui::diffs::{DiffViewer, ProductDiffs};

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
                    if let Some(b) = self.local_products.badges.get_mut(&slug) {
                        b.id = Some(new.id);
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
        let upload_product =
            async |universe_id: u64, product: Product, product_type: ProductType| -> Result<u64> {
                let update_request = ProductUpdateRequest::from(&product);
                let icon_path = product.icon.as_deref();
                let product_id = match product_type {
                    ProductType::Pass => {
                        create_pass(universe_id, &update_request, icon_path)
                            .await?
                            .game_pass_id
                    }

                    ProductType::DevProduct => {
                        create_dev_product(universe_id, &update_request, icon_path)
                            .await?
                            .product_id
                    }

                    ProductType::Badge => {
                        return Err("badge creates handled by upload_badge_creates".into());
                    }
                };

                info!(
                    "uploaded {:?} '{}' with id {}",
                    product_type, product.name, product_id
                );

                Ok(product_id)
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
                let mut pass = pass.clone();

                apply_discount_prefix(
                    &mut pass,
                    self.local_products.metadata.discount_prefix.clone(),
                );

                let future = (async move {
                    let product_id =
                        upload_product(universe_id, pass, ProductType::Pass).await;

                    match product_id {
                        Ok(id) => Some((name, id)),
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
                let mut devproduct = devproduct.clone();

                apply_discount_prefix(
                    &mut devproduct,
                    self.local_products.metadata.discount_prefix.clone(),
                );

                let future = (async move {
                    let product_id =
                        upload_product(universe_id, devproduct.clone(), ProductType::DevProduct)
                            .await;

                    match product_id {
                        Ok(id) => Some((name, id)),
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
            if let Some((name, id)) = res {
                self.local_products
                    .passes
                    .get_mut(name.as_str())
                    .unwrap()
                    .id = Some(id as u64);
            }
        });

        devproduct_futures.into_iter().for_each(|res| {
            if let Some((name, id)) = res {
                self.local_products
                    .products
                    .get_mut(name.as_str())
                    .unwrap()
                    .id = Some(id as u64);
            }
        });

        self.local_products.save_products().await?;
        self.local_products.serialize_luau().await?;

        Ok(())
    }

    async fn upload_modified(&mut self, overwrite: bool, auto_confirm: bool) -> Result<()> {
        let mut product_diffs = vec![];

        let universe_id = self.local_products.metadata.universe_id.clone();
        let products = &self.remote_products;
        let mut all_local_products = vec![];

        all_local_products.extend(self.local_products.passes.values().cloned());
        all_local_products.extend(self.local_products.products.values().cloned());
        all_local_products.extend(self.local_products.badges.values().cloned());

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

                    local_product
                        .diff(&remote_product, Some(&self.local_products.metadata))
                        .map(|diff| (product_type, diff))
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
            let icon_path = local_product.icon.as_deref();

            let result: Result<()> = match product_type {
                ProductType::Pass => {
                    update_pass(universe_id, id, &update_request, icon_path).await
                }
                ProductType::DevProduct => {
                    update_dev_product(universe_id, id, &update_request, icon_path).await
                }
                ProductType::Badge => {
                    let badge_request = BadgeUpdateRequest::from(&local_product);
                    update_badge(id, &badge_request).await
                }
            };

            match result {
                Ok(()) => {
                    succeeded += 1;
                    info!("synced {:?} '{}' (id: {})", product_type, name, id);
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

    pub async fn upload(overwrite: bool, auto_confirm: bool) -> Result<()> {
        let mut uploader = Uploader::create().await?;
        crate::alpha_bleed::set_bleed_enabled(uploader.local_products.icons.bleed);

        let mut run_upload = async || -> Result<()> {
            uploader.upload_empty(overwrite).await?;
            uploader.upload_modified(overwrite, auto_confirm).await?;

            Ok(())
        };

        let upload_result = run_upload().await;

        uploader.local_products.save_products().await?;
        uploader.local_products.serialize_luau().await?;

        if let Err(e) = upload_result {
            info!("failed to upload modified products: {}, aborting upload", e);
            return Err(e);
        }

        Ok(())
    }
}
