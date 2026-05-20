use nestify::nest;
use reqwest::multipart::Form;
use serde::{Deserialize, Serialize};

use crate::sync::products::Product;

macro_rules! paginate_struct {
    ($type:ty, $name:ident, $field:ident) => {
        #[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            #[serde(default)]
            pub $field: Vec<$type>,
            #[serde(default, alias = "nextPageCursor", alias = "next_page_cursor")]
            pub next_page_token: Option<String>,
        }
    };
}

nest! {
    #[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]*
    #[serde(rename_all = "camelCase")]*
    pub struct DevProduct {
        pub product_id: u64,
        pub name: String,
        pub description: String,
        pub universe_id: u64,
        pub is_for_sale: bool,
        pub store_page_enabled: bool,
        pub price_information: Option<pub struct ProductPriceInformation{
            pub default_price_in_robux: u64,
            pub enabled_features: Option<Vec<String>>,
        }>,
        pub is_immutable: bool,
        pub created_timestamp: String,
        pub updated_timestamp: String,
        #[serde(default, alias = "iconAssetId")]
        pub icon_image_asset_id: Option<u64>,
    }
}

nest! {
    #[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]*
    #[serde(rename_all = "camelCase")]*
    pub struct GamePass {
        pub game_pass_id: u64,
        pub name: String,
        pub description: String,
        pub is_for_sale: bool,
        pub icon_asset_id: u64,
        pub created_timestamp: String,
        pub updated_timestamp: String,
        pub price_information: Option<pub struct PriceInformation {
            pub default_price_in_robux: u64,
            pub enabled_features: Option<Vec<String>>,
        }>,
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductUpdateRequest {
    pub name: String,
    pub description: Option<String>,
    pub is_for_sale: Option<bool>,
    pub price: Option<u64>,
    pub is_regional_pricing_enabled: Option<bool>,
    pub store_page_enabled: Option<bool>,
}

mod lenient_u64 {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Num(u64),
            Str(String),
        }

        match Repr::deserialize(deserializer)? {
            Repr::Num(n) => Ok(n),
            Repr::Str(s) => s.parse::<u64>().map_err(serde::de::Error::custom),
        }
    }
}

nest! {
    #[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]*
    #[serde(rename_all = "camelCase")]*
    pub struct Badge {
        #[serde(default, deserialize_with = "lenient_u64::deserialize")]
        pub id: u64,
        #[serde(default)]
        pub name: String,
        #[serde(default)]
        pub description: Option<String>,
        #[serde(default)]
        pub enabled: bool,
        #[serde(default)]
        pub icon_image_id: Option<u64>,
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BadgeMetadata {
    #[serde(default)]
    pub badge_creation_price: u64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BadgePage {
    #[serde(default)]
    pub data: Vec<Badge>,
    #[serde(default, alias = "nextPageToken", alias = "next_page_token")]
    pub next_page_cursor: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BadgeUpdateRequest {
    pub name: String,
    pub description: String,
    pub enabled: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BadgeIconResponse {
    #[serde(default, alias = "iconImageId", alias = "imageAssetId")]
    pub icon_image_id: Option<u64>,
}

paginate_struct!(DevProduct, DevProductPage, developer_products);
paginate_struct!(GamePass, GamePassPage, game_passes);

impl From<&Product> for ProductUpdateRequest {
    fn from(p: &Product) -> Self {
        Self {
            name: p.get_title(),
            description: p.description.clone(),
            is_for_sale: Some(p.active),
            price: Some(p.get_price() as u64),
            is_regional_pricing_enabled: p.regional_pricing,
            store_page_enabled: None,
        }
    }
}

impl From<&GamePass> for ProductUpdateRequest {
    fn from(gp: &GamePass) -> Self {
        let product = Product::from(gp);
        ProductUpdateRequest::from(&product)
    }
}

impl From<&DevProduct> for ProductUpdateRequest {
    fn from(dp: &DevProduct) -> Self {
        let product = Product::from(dp);
        ProductUpdateRequest::from(&product)
    }
}

impl From<&GamePass> for Product {
    fn from(gp: &GamePass) -> Self {
        let features = gp
            .price_information
            .as_ref()
            .map_or(None, |pi| pi.enabled_features.clone());

        Self {
            discount: None,
            prefix: None,
            id: Some(gp.game_pass_id as u64),
            name: gp.name.clone(),
            description: Some(gp.description.clone()),
            active: gp.is_for_sale,
            price: gp
                .price_information
                .as_ref()
                .map_or(0, |pi| pi.default_price_in_robux as i64),
            regional_pricing: features.map(|f| f.iter().any(|i| i == "RegionalPricing")),
            icon: None,
            icon_id: Some(gp.icon_asset_id),
            icon_hash: None,
            path: None,
        }
    }
}

impl From<&Badge> for Product {
    fn from(b: &Badge) -> Self {
        Self {
            discount: None,
            prefix: None,
            id: Some(b.id),
            name: b.name.clone(),
            description: Some(b.description.clone().unwrap_or_default()),
            active: b.enabled,
            price: 0,
            regional_pricing: None,
            icon: None,
            icon_id: b.icon_image_id,
            icon_hash: None,
            path: None,
        }
    }
}

impl From<&Product> for BadgeUpdateRequest {
    fn from(p: &Product) -> Self {
        Self {
            name: p.get_title(),
            description: p.description.clone().unwrap_or_default(),
            enabled: p.active,
        }
    }
}

impl From<&DevProduct> for Product {
    fn from(dp: &DevProduct) -> Self {
        let features = dp
            .price_information
            .as_ref()
            .map_or(None, |pi| pi.enabled_features.clone());

        Self {
            discount: None,
            prefix: None,
            id: Some(dp.product_id as u64),
            name: dp.name.clone(),
            description: Some(dp.description.clone()),
            active: dp.is_for_sale,
            price: dp
                .price_information
                .as_ref()
                .map_or(0, |pi| pi.default_price_in_robux as i64),
            regional_pricing: features.map(|f| f.iter().any(|i| i == "RegionalPricing")),
            icon: None,
            icon_id: dp.icon_image_asset_id,
            icon_hash: None,
            path: None,
        }
    }
}

impl From<&ProductUpdateRequest> for Form {
    fn from(update: &ProductUpdateRequest) -> Self {
        let mut form = Form::new().text("name", update.name.clone());

        if let Some(description) = &update.description {
            form = form.text("description", description.clone());
        }

        if let Some(is_for_sale) = update.is_for_sale {
            form = form.text("isForSale", is_for_sale.to_string());
        }

        if let Some(price) = update.price
            && price > 0
        {
            form = form.text("price", price.to_string());
        }

        if let Some(is_regional_pricing_enabled) = update.is_regional_pricing_enabled {
            form = form.text(
                "isRegionalPricingEnabled",
                is_regional_pricing_enabled.to_string(),
            );
        }

        form
    }
}
