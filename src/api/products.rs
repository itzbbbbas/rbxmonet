use log::{info, warn};
use reqwest::{Response, StatusCode};

use super::model::{Badge, DevProduct, GamePass};
use super::open_cloud_client;

const CREDENTIALS_URL: &str = "https://create.roblox.com/dashboard/credentials";

/// Open Cloud API system + the scope(s) an operation needs. Used to turn a
/// bare 401/403 into a message that names exactly what to enable on the key.
struct RequiredPermission {
    api_system: &'static str,
    scopes: &'static [&'static str],
}

/// Map a human operation name (the `op` passed to [`check_status`]) to the
/// API system and scope it requires. Returns `None` for operations not in
/// the table so the caller can fall back to a generic message.
fn required_permission(op: &str) -> Option<RequiredPermission> {
    let (api_system, scopes): (&str, &[&str]) = match op {
        "list passes" => ("game-passes", &["game-pass:read"]),
        "create pass" | "update pass" => ("game-passes", &["game-pass:write"]),
        "list dev products" => ("developer-products", &["developer-product:read"]),
        "create dev product" | "update dev product" => {
            ("developer-products", &["developer-product:write"])
        }
        "list badges" | "fetch badge metadata" | "fetch free-badges-quota" | "create badge"
        | "update badge" | "update badge icon" => ("legacy-badges", &["legacy-badge:manage"]),
        _ => return None,
    };
    Some(RequiredPermission { api_system, scopes })
}

/// Pull the human-readable message out of a Roblox error body
/// (`{"errors":[{"message":"..."}]}` or `{"message":"..."}`), falling back to
/// the raw body when it isn't recognizable JSON.
fn extract_server_message(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(msg) = value
            .get("errors")
            .and_then(|e| e.as_array())
            .and_then(|a| a.first())
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
        {
            return msg.to_string();
        }
        if let Some(msg) = value.get("message").and_then(|m| m.as_str()) {
            return msg.to_string();
        }
    }
    body.to_string()
}

/// Build a friendly, actionable message for a 401/403 that tells the user
/// which Open Cloud permission the operation needs and how to grant it.
fn unauthorized_message(op: &str, status: StatusCode, server_msg: &str) -> String {
    let mut out = format!("{op}: Roblox rejected the request (HTTP {status}).\n");

    match required_permission(op) {
        Some(perm) => {
            let scope_list = perm.scopes.join(", ");
            out.push_str(&format!(
                "\nThis operation needs the `{}` scope under the \"{}\" API system, \
                 but your API key is missing it (or the key is invalid / expired / IP-restricted).\n",
                scope_list, perm.api_system
            ));
            out.push_str("\nHow to fix:\n");
            out.push_str(&format!("  1. Open {CREDENTIALS_URL}\n"));
            out.push_str("  2. Edit your API key and expand \"Access Permissions\".\n");
            out.push_str(&format!(
                "  3. Under \"{}\", add: {}\n",
                perm.api_system, scope_list
            ));
            out.push_str(
                "  4. Make sure the key is enabled for this experience and not blocked by IP restrictions.\n",
            );
        }
        None => {
            out.push_str(
                "\nYour API key was rejected. Check that it is valid, not expired, has the \
                 permission this operation needs, is enabled for this experience, and is not \
                 blocked by IP restrictions.\n",
            );
            out.push_str(&format!("  Manage keys: {CREDENTIALS_URL}\n"));
        }
    }

    if !server_msg.is_empty() {
        out.push_str(&format!("\nServer response: {server_msg}"));
    }
    out
}

async fn check_status(resp: Response, op: &str) -> Result<Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    let trimmed = body.trim();

    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        let server_msg = extract_server_message(trimmed);
        return Err(unauthorized_message(op, status, &server_msg).into());
    }

    if trimmed.is_empty() {
        Err(format!("{op}: HTTP {status}").into())
    } else {
        Err(format!("{op}: HTTP {status} \u{2014} {trimmed}").into())
    }
}

async fn json_or_explain<T: serde::de::DeserializeOwned>(
    resp: Response,
    op: &str,
) -> Result<T> {
    let body = resp.text().await?;
    serde_json::from_str::<T>(&body).map_err(|e| {
        let preview: String = body.chars().take(500).collect();
        format!("{op}: decoding response body failed: {e} \u{2014} body[..500]: {preview}").into()
    })
}

use crate::Result;
use crate::api::model::{
    BadgeIconResponse, BadgeMetadata, BadgePage, BadgeUpdateRequest, DevProductPage, GamePassPage,
    ProductUpdateRequest,
};
use crate::sync::products::{MultiProduct, Product};

/// Snapshot of remote products for a universe. Each `*_fetched` flag is
/// true when the endpoint replied (zero-length vec counts as a successful
/// "nothing in universe"), false when the call was skipped due to
/// missing auth / unavailable endpoint. Pruning logic uses these flags
/// to avoid nuking a local section when its remote fetch was skipped.
pub struct RemoteSnapshot {
    pub products: Vec<MultiProduct>,
    pub passes_fetched: bool,
    pub dev_products_fetched: bool,
    pub badges_fetched: bool,
}

pub async fn fetch_all_products(universe_id: u64) -> Result<RemoteSnapshot> {
    let passes = fetch_all_passes(universe_id).await?;
    let products = fetch_all_dev_products(universe_id).await?;
    let badges = fetch_all_badges(universe_id).await?;

    let mut all_products: Vec<MultiProduct> = Vec::new();

    if let Some(items) = &passes {
        all_products.extend(
            items
                .iter()
                .map(|x| MultiProduct::Pass(Product::from(x))),
        );
    }
    if let Some(items) = &products {
        all_products.extend(
            items
                .iter()
                .map(|x| MultiProduct::DevProduct(Product::from(x))),
        );
    }
    if let Some(items) = &badges {
        all_products.extend(items.iter().map(|x| MultiProduct::Badge(Product::from(x))));
    }

    Ok(RemoteSnapshot {
        products: all_products,
        passes_fetched: passes.is_some(),
        dev_products_fetched: products.is_some(),
        badges_fetched: badges.is_some(),
    })
}

pub async fn fetch_all_badges(universe_id: u64) -> Result<Option<Vec<Badge>>> {
    let mut badges = vec![];

    let limit = 100;
    let mut cursor: String = String::default();

    loop {
        let url = format!(
            "https://badges.roblox.com/v1/universes/{}/badges",
            universe_id
        );
        let mut req = open_cloud_client()
            .get(&url)
            .query(&[("limit", limit.to_string()), ("sortOrder", "Asc".to_string())]);

        if !cursor.is_empty() {
            req = req.query(&[("cursor", cursor.clone())]);
        }

        let resp = req.send().await?;
        let status = resp.status();

        if status == reqwest::StatusCode::NOT_FOUND
            || status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::UNAUTHORIZED
        {
            warn!(
                "badges endpoint returned {} for universe {} \u{2014} skipping badges \
                 (key likely missing the `legacy-badge:manage` scope under \"legacy-badges\", \
                 or not enabled for this experience)",
                status, universe_id
            );
            return Ok(None);
        }

        let resp = check_status(resp, "list badges").await?;
        let page: BadgePage = json_or_explain(resp, "list badges").await?;
        let got = page.data.len();
        badges.extend(page.data);
        log::debug!(
            "badges page: +{} (total {}), next_cursor={:?}",
            got,
            badges.len(),
            page.next_page_cursor.as_deref()
        );

        match page.next_page_cursor {
            Some(c) if !c.is_empty() => {
                cursor = c;
            }
            _ => break,
        }
    }

    info!("fetched {} badges", badges.len());
    Ok(Some(badges))
}

pub async fn update_badge(badge_id: u64, update: &BadgeUpdateRequest) -> Result<()> {
    let url = format!(
        "https://apis.roblox.com/legacy-badges/v1/badges/{}",
        badge_id
    );
    let resp = open_cloud_client().patch(&url).json(update).send().await?;

    check_status(resp, "update badge").await?;

    Ok(())
}

pub async fn fetch_badge_metadata() -> Result<BadgeMetadata> {
    let url = "https://badges.roblox.com/v1/badges/metadata";
    let resp = open_cloud_client().get(url).send().await?;
    let resp = check_status(resp, "fetch badge metadata").await?;
    json_or_explain(resp, "fetch badge metadata").await
}

pub async fn fetch_free_badges_quota(universe_id: u64) -> Result<u64> {
    let url = format!(
        "https://badges.roblox.com/v1/universes/{}/free-badges-quota",
        universe_id
    );
    let resp = open_cloud_client().get(&url).send().await?;
    let resp = check_status(resp, "fetch free-badges-quota").await?;
    let body: serde_json::Value = json_or_explain(resp, "fetch free-badges-quota").await?;
    Ok(body
        .get("quota")
        .and_then(|v| v.as_u64())
        .or_else(|| body.as_u64())
        .unwrap_or(0))
}

pub async fn create_badge(
    universe_id: u64,
    name: &str,
    description: &str,
    is_active: bool,
    icon_path: &str,
    expected_cost: u64,
) -> Result<crate::api::model::Badge> {
    let (bytes, filename, mime) = prepare_icon_bytes(icon_path).await?;
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename)
        .mime_str(mime)?;
    let form = reqwest::multipart::Form::new()
        .text("name", name.to_string())
        .text("description", description.to_string())
        .text("paymentSourceType", "1")
        .text("expectedCost", expected_cost.to_string())
        .text("isActive", is_active.to_string())
        .part("files", part);

    let url = format!(
        "https://apis.roblox.com/legacy-badges/v1/universes/{}/badges",
        universe_id
    );
    let resp = open_cloud_client()
        .post(&url)
        .multipart(form)
        .send()
        .await?;
    let resp = check_status(resp, "create badge").await?;
    let badge: crate::api::model::Badge = json_or_explain(resp, "create badge").await?;
    Ok(badge)
}

pub async fn fetch_all_dev_products(universe_id: u64) -> Result<Option<Vec<DevProduct>>> {
    let mut products = vec![];

    let page_size = 100;
    let mut page_cursor: String = String::default();

    loop {
        let url = format!(
            "https://apis.roblox.com/developer-products/v2/universes/{}/developer-products/creator",
            universe_id
        );
        let mut req = open_cloud_client()
            .get(&url)
            .query(&[("pageSize", page_size.to_string())]);

        if !page_cursor.is_empty() {
            req = req.query(&[("pageToken", page_cursor.clone())]);
        }

        let resp = check_status(req.send().await?, "list dev products").await?;
        let page: DevProductPage = json_or_explain(resp, "list dev products").await?;

        let got = page.developer_products.len();
        products.extend(page.developer_products);
        log::debug!(
            "dev products page: +{} (total {}), next_token={:?}",
            got,
            products.len(),
            page.next_page_token.as_deref()
        );

        match page.next_page_token {
            Some(cursor) if !cursor.is_empty() => {
                page_cursor = cursor;
            }
            _ => break,
        }
    }

    info!("fetched {} dev products", products.len());
    Ok(Some(products))
}

pub async fn fetch_all_passes(universe_id: u64) -> Result<Option<Vec<GamePass>>> {
    let mut passes = vec![];

    let page_size = 100;
    let mut page_cursor: String = String::default();

    loop {
        let url = format!(
            "https://apis.roblox.com/game-passes/v1/universes/{}/game-passes/creator",
            universe_id
        );
        let mut req = open_cloud_client()
            .get(&url)
            .query(&[("pageSize", page_size.to_string())]);

        if !page_cursor.is_empty() {
            req = req.query(&[("pageToken", page_cursor.clone())]);
        }

        let resp = check_status(req.send().await?, "list passes").await?;
        let page: GamePassPage = json_or_explain(resp, "list passes").await?;

        let got = page.game_passes.len();
        passes.extend(page.game_passes);
        log::debug!(
            "passes page: +{} (total {}), next_token={:?}",
            got,
            passes.len(),
            page.next_page_token.as_deref()
        );

        match page.next_page_token {
            Some(cursor) if !cursor.is_empty() => {
                page_cursor = cursor;
            }
            _ => break,
        }
    }

    info!("fetched {} passes", passes.len());
    Ok(Some(passes))
}

async fn attach_icon_part(
    form: reqwest::multipart::Form,
    field: &str,
    icon_path: Option<&str>,
) -> Result<reqwest::multipart::Form> {
    let Some(path) = icon_path else { return Ok(form) };
    let (bytes, filename, mime) = match prepare_icon_bytes(path).await {
        Ok(t) => t,
        Err(e) => {
            warn!(
                "icon '{}' unreadable \u{2014} skipping icon upload ({})",
                path, e
            );
            return Ok(form);
        }
    };
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename)
        .mime_str(mime)?;
    Ok(form.part(field.to_string(), part))
}

pub async fn update_dev_product(
    universe_id: u64,
    product_id: u64,
    update: &ProductUpdateRequest,
    icon_path: Option<&str>,
) -> Result<Option<u64>> {
    let url = format!(
        "https://apis.roblox.com/developer-products/v2/universes/{}/developer-products/{}",
        universe_id, product_id
    );
    let form: reqwest::multipart::Form = update.into();
    let form = attach_icon_part(form, "imageFile", icon_path).await?;
    let resp = open_cloud_client()
        .patch(&url)
        .multipart(form)
        .send()
        .await?;

    let resp = check_status(resp, "update dev product").await?;
    let parsed: std::result::Result<DevProduct, _> =
        json_or_explain(resp, "update dev product").await;
    Ok(parsed.ok().and_then(|dp| dp.icon_image_asset_id))
}

pub async fn update_pass(
    universe_id: u64,
    pass_id: u64,
    update: &ProductUpdateRequest,
    icon_path: Option<&str>,
) -> Result<Option<u64>> {
    let url = format!(
        "https://apis.roblox.com/game-passes/v1/universes/{}/game-passes/{}",
        universe_id, pass_id
    );
    let form: reqwest::multipart::Form = update.into();
    let form = attach_icon_part(form, "file", icon_path).await?;
    let resp = open_cloud_client()
        .patch(&url)
        .multipart(form)
        .send()
        .await?;

    let resp = check_status(resp, "update pass").await?;
    let parsed: std::result::Result<GamePass, _> = json_or_explain(resp, "update pass").await;
    Ok(parsed.ok().map(|gp| gp.icon_asset_id))
}

/// POST the legacy-publish icon update endpoint for a badge. Returns the
/// new icon image asset id when the response carries one.
pub async fn update_badge_icon(badge_id: u64, icon_path: &str) -> Result<Option<u64>> {
    let (bytes, filename, mime) = prepare_icon_bytes(icon_path).await?;
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename)
        .mime_str(mime)?;
    let form = reqwest::multipart::Form::new().part("request", part);

    let url = format!(
        "https://apis.roblox.com/legacy-publish/v1/badges/{}/icon",
        badge_id
    );
    let resp = open_cloud_client()
        .post(&url)
        .multipart(form)
        .send()
        .await?;
    let resp = check_status(resp, "update badge icon").await?;
    let parsed: std::result::Result<BadgeIconResponse, _> =
        json_or_explain(resp, "update badge icon").await;
    Ok(parsed.ok().and_then(|b| b.icon_image_id))
}

/// BLAKE3 hash of an icon file's raw on-disk bytes. Returns None when the
/// file is unreadable so callers can degrade gracefully rather than abort.
pub async fn hash_icon_file(icon_path: &str) -> Option<String> {
    match tokio::fs::read(icon_path).await {
        Ok(bytes) => Some(blake3::hash(&bytes).to_hex().to_string()),
        Err(e) => {
            log::warn!("icon '{}' unreadable for hashing: {}", icon_path, e);
            None
        }
    }
}

pub async fn create_dev_product(
    universe_id: u64,
    product: &ProductUpdateRequest,
    icon_path: Option<&str>,
) -> Result<DevProduct> {
    let url = format!(
        "https://apis.roblox.com/developer-products/v2/universes/{}/developer-products",
        universe_id
    );
    let form: reqwest::multipart::Form = product.into();
    let form = attach_icon_part(form, "imageFile", icon_path).await?;
    let resp = open_cloud_client()
        .post(&url)
        .multipart(form)
        .send()
        .await?;

    let resp = check_status(resp, "create dev product").await?;
    Ok(resp.json().await?)
}

pub async fn create_pass(
    universe_id: u64,
    pass: &ProductUpdateRequest,
    icon_path: Option<&str>,
) -> Result<GamePass> {
    let url = format!(
        "https://apis.roblox.com/game-passes/v1/universes/{}/game-passes",
        universe_id
    );
    let form: reqwest::multipart::Form = pass.into();
    let form = attach_icon_part(form, "imageFile", icon_path).await?;
    let resp = open_cloud_client()
        .post(&url)
        .multipart(form)
        .send()
        .await?;

    let resp = check_status(resp, "create pass").await?;
    Ok(resp.json().await?)
}

const MAX_ICON_DIM: u32 = 512;
const RESIZE_TARGET_DIM: u32 = 256;

/// Load an icon file and prepare bytes for upload. Roblox rejects badge
/// icons larger than ~512x512; auto-resize to 256x256 PNG when needed so
/// users can point at any image without manual resizing. Returns
/// (bytes, filename, mime_type).
async fn prepare_icon_bytes(icon_path: &str) -> Result<(Vec<u8>, String, &'static str)> {
    let raw = tokio::fs::read(icon_path)
        .await
        .map_err(|e| format!("read icon file '{}': {}", icon_path, e))?;

    let cursor = std::io::Cursor::new(&raw);
    let reader = image::ImageReader::new(cursor)
        .with_guessed_format()
        .map_err(|e| format!("guess icon format '{}': {}", icon_path, e))?;
    let img = reader
        .decode()
        .map_err(|e| format!("decode icon '{}': {}", icon_path, e))?;

    let (w, h) = (img.width(), img.height());
    let needs_resize = w > MAX_ICON_DIM || h > MAX_ICON_DIM;

    let p = std::path::Path::new(icon_path);
    let base_name = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("icon")
        .to_string();

    if needs_resize {
        log::info!(
            "resizing icon '{}' from {}x{} to {}x{} for Roblox upload limit",
            icon_path, w, h, RESIZE_TARGET_DIM, RESIZE_TARGET_DIM
        );
    }

    let processed = if needs_resize {
        img.resize(
            RESIZE_TARGET_DIM,
            RESIZE_TARGET_DIM,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };
    let mut rgba = image::DynamicImage::ImageRgba8(processed.to_rgba8());

    if crate::alpha_bleed::bleed_enabled() {
        crate::alpha_bleed::alpha_bleed(&mut rgba);
    }

    let mut buf = Vec::with_capacity(64 * 1024);
    rgba.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| format!("encode icon '{}' as png: {}", icon_path, e))?;
    Ok((buf, format!("{}.png", base_name), "image/png"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_op_names_read_scope() {
        let msg = unauthorized_message("list passes", StatusCode::UNAUTHORIZED, "Invalid API Key");
        assert!(msg.contains("game-pass:read"), "msg: {msg}");
        assert!(msg.contains("game-passes"), "msg: {msg}");
        assert!(msg.contains("Invalid API Key"), "msg: {msg}");
        assert!(msg.contains(CREDENTIALS_URL), "msg: {msg}");
        assert!(msg.contains("401"), "msg: {msg}");
    }

    #[test]
    fn mutation_op_names_write_scope() {
        let msg = unauthorized_message("create pass", StatusCode::FORBIDDEN, "");
        assert!(msg.contains("game-pass:write"), "msg: {msg}");
        assert!(!msg.contains("Server response"), "msg: {msg}");
    }

    #[test]
    fn dev_product_ops_map_correctly() {
        assert!(
            unauthorized_message("list dev products", StatusCode::UNAUTHORIZED, "")
                .contains("developer-product:read")
        );
        assert!(
            unauthorized_message("update dev product", StatusCode::FORBIDDEN, "")
                .contains("developer-product:write")
        );
    }

    #[test]
    fn badge_ops_map_to_legacy_scope() {
        let msg = unauthorized_message("create badge", StatusCode::UNAUTHORIZED, "");
        assert!(msg.contains("legacy-badge:manage"), "msg: {msg}");
        assert!(msg.contains("legacy-badges"), "msg: {msg}");
    }

    #[test]
    fn unknown_op_falls_back_to_generic() {
        let msg = unauthorized_message("frobnicate", StatusCode::UNAUTHORIZED, "boom");
        assert!(msg.contains("API key was rejected"), "msg: {msg}");
        assert!(msg.contains("boom"), "msg: {msg}");
    }

    #[test]
    fn extracts_nested_error_message() {
        let body = r#"{"errors":[{"code":0,"message":"Invalid API Key"}]}"#;
        assert_eq!(extract_server_message(body), "Invalid API Key");
    }

    #[test]
    fn extracts_top_level_message() {
        assert_eq!(extract_server_message(r#"{"message":"Forbidden"}"#), "Forbidden");
    }

    #[test]
    fn falls_back_to_raw_body_when_not_json() {
        assert_eq!(extract_server_message("not json"), "not json");
    }
}
