use clap::{Parser, Subcommand};
use log::info;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

mod alpha_bleed;
mod api;
pub mod sync;
pub mod ui;
pub mod utils;

use crate::sync::download::Downloader;
use crate::sync::upload::Uploader;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
    /// Automatically answer "yes" to all prompts
    #[arg(short = 'y', long, default_value_t = false)]
    yes: bool,
    #[arg(short = 'o', long, default_value_t = false)]
    overwrite: bool,
    /// Skip diff viewer in `sync` — auto-confirm all changes
    #[arg(short = 'a', long, default_value_t = false, global = true)]
    auto_confirm: bool,
    /// Force re-upload of every entry's icon during `sync` (even when no other field diffs).
    /// Roblox API does not expose icon comparison, so icon-only edits otherwise get skipped.
    #[arg(long = "force-icons", default_value_t = false, global = true)]
    force_icons: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initializes the products file
    Init,
    /// Downloads all the products from the universe
    Download,
    /// Syncs products between file and universe
    Sync,
    /// Regenerates the Luau file from rbxmonet.toml without any network calls
    RegenLuau,
}

fn env_truthy(key: &str) -> bool {
    std::env::var(key)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

fn init_logging() {
    if std::env::var("RUST_LOG").is_err() {
        let crate_name = env!("CARGO_PKG_NAME");
        let filter = if cfg!(debug_assertions) {
            format!("off,{crate_name}=debug")
        } else {
            format!("{crate_name}=info")
        };
        unsafe { std::env::set_var("RUST_LOG", filter) }
    }

    env_logger::init();
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    init_logging();
    let _ = color_eyre::install();

    if std::env::var("RBX_MONET_API_KEY").ok().is_some() {
        log::warn!(
            "RBX_MONET_API_KEY is deprecated \u{2014} rename to RBXMONET_API_KEY (rbxmonet >= 0.1.25)"
        );
    }
    let api_key_set = std::env::var("RBXMONET_API_KEY")
        .ok()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    info!(
        "auth: RBXMONET_API_KEY={}",
        if api_key_set { "set" } else { "MISSING" }
    );
    if api_key_set {
        if let Some(token) = std::env::var("RBXMONET_API_KEY").ok() {
            api::set_api_token(token).await;
        }
    }

    let args = Args::parse();
    let command = match args.command {
        Some(cmd) => cmd,
        None => {
            eprintln!("No command provided. Use --help for more information.");
            return;
        }
    };

    // flags::FLAGS.auto_yes = args.yes;

    let result = match command {
        Commands::Init => {
            info!("Initializing products file...");

            if std::fs::exists("rbxmonet.toml").unwrap() {
                log::error!("rbxmonet.toml already exists. Aborting initialization.");
                return;
            }

            let template = include_str!("templates/rbxmonet.toml.template");
            match std::fs::write("rbxmonet.toml", template) {
                Ok(_) => {
                    info!("rbxmonet.toml initialized successfully.");
                    Ok(())
                }
                Err(e) => Err(format!("Failed to initialize rbxmonet.toml: {}", e).into()),
            }
        }
        Commands::Download => Downloader::download(args.overwrite).await,
        Commands::Sync => {
            if std::env::var("RBX_MONET_AUTO_CONFIRM").ok().is_some() {
                log::warn!(
                    "RBX_MONET_AUTO_CONFIRM is deprecated \u{2014} rename to RBXMONET_AUTO_CONFIRM"
                );
            }
            let auto_confirm = args.auto_confirm || env_truthy("RBXMONET_AUTO_CONFIRM");
            Uploader::upload(args.overwrite, auto_confirm, args.force_icons).await
        }
        Commands::RegenLuau => {
            info!("Regenerating Luau file from rbxmonet.toml...");
            match sync::products::VCSProducts::get_products().await {
                Ok(products) => match products.serialize_luau().await {
                    Ok(_) => {
                        info!("Luau file regenerated successfully.");
                        Ok(())
                    }
                    Err(e) => Err(format!("Failed to serialize Luau: {}", e).into()),
                },
                Err(e) => Err(format!("Failed to read rbxmonet.toml: {}", e).into()),
            }
        }
    };

    if let Err(e) = result {
        log::error!("Error: {}", e);
        eprintln!("error: {e}");
    }
}
