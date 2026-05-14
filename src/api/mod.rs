use std::sync::Arc;

use reqwest::{Client, Url, cookie::Jar};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
// use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use tokio::sync::Mutex;

use crate::api::middleware::{
    RobloxAuthMiddleware, RobloxRateLimitMiddleware, RobloxXsrfMiddleware,
};

mod middleware;
pub mod model;
pub mod products;

lazy_static::lazy_static! {
    static ref API_TOKEN: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    static ref API_CLIENT: ClientWithMiddleware = {
        // let retry_policy = ExponentialBackoff::builder()
        //         .build_with_max_retries(5);

        let jar = Arc::new(Jar::default());

        if let Ok(cookie) = std::env::var("RBX_MONET_ROBLOSECURITY") {
            let cookie = cookie.trim();
            if !cookie.is_empty() {
                let url: Url = "https://www.roblox.com/".parse().unwrap();
                jar.add_cookie_str(
                    &format!(".ROBLOSECURITY={cookie}; Domain=.roblox.com; Path=/"),
                    &url,
                );
            }
        }

        let client = Client::builder()
            .user_agent(format!("rbxmonet/{}", env!("CARGO_PKG_VERSION")))
            .cookie_provider(jar)
            .build().unwrap();

        ClientBuilder::new(client)
            .with(RobloxAuthMiddleware::new())
            .with(RobloxXsrfMiddleware::new())
            .with(RobloxRateLimitMiddleware::new().with_max_429_retries(5))
            // .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build()
    };
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
