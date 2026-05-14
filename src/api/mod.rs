use std::sync::Arc;

use reqwest::Client;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use tokio::sync::Mutex;

use crate::api::middleware::{RobloxAuthMiddleware, RobloxRateLimitMiddleware};

mod middleware;
pub mod model;
pub mod products;

lazy_static::lazy_static! {
    static ref API_TOKEN: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    static ref OPEN_CLOUD_CLIENT: ClientWithMiddleware = build_open_cloud_client();
}

fn build_open_cloud_client() -> ClientWithMiddleware {
    let client = Client::builder()
        .user_agent(format!("rbxmonet/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap();

    ClientBuilder::new(client)
        .with(RobloxAuthMiddleware::new())
        .with(RobloxRateLimitMiddleware::new().with_max_429_retries(5))
        .build()
}

pub fn open_cloud_client() -> &'static ClientWithMiddleware {
    &OPEN_CLOUD_CLIENT
}

#[macro_export]
macro_rules! multipart_form {
	($($key:expr => $value:expr),* $(,)?) => {
		{
			let mut form = reqwest::multipart::Form::new();
			$(
				form = form.text($key, $value);
			)*
			form
		}
	};
}

pub async fn set_api_token(token: String) {
    let mut guard = API_TOKEN.lock().await;
    *guard = Some(token);
}
