use anyhow::{anyhow, Context};

use proglad_server::config::{self, Config};
use proglad_server::server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default())
        .filter_module("sqlx", log::LevelFilter::Error)
        .init();
    let args: Vec<String> = std::env::args().collect();
    let config: Config = if args.len() > 1 {
        let config = tokio::fs::read_to_string(&args[1])
            .await
            .context(format!("Failed to read config file {}", args[1]))?;
        let mut insecure = config::Insecure::Deny;
        for f in args[2..].iter() {
            match f.as_str() {
                "--insecure" => insecure = config::Insecure::Allow,
                _ => return Err(anyhow!("Unrecognized flag: {f}")),
            }
        }
        let config = toml::from_str(&config).context("Failed to parse config")?;
        config::validate(&config, insecure)
            .map_err(|e| anyhow!("Config validation failed: {e}"))?;
        config
    } else {
        return Err(anyhow::Error::msg(
            "config file must be specified as the first arg",
        ));
    };
    server::run(config).await
}
