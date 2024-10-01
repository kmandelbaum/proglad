use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct AccessControl {
    #[serde(default)]
    pub insecure_default_account: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServerConfig {
    pub port: u16,
    pub site_base_url_path: String,
    pub auth_base_url: String,
    pub kratos_api_url: String,
    #[serde(default)]
    pub fs_root_dir: std::path::PathBuf,

    #[serde(default)]
    pub access_control: AccessControl,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub server_config: ServerConfig,
    pub manager_config: proglad_controller::manager::Config,
    pub match_runner_config: proglad_controller::match_runner::Config,
    pub scheduler_config: crate::scheduler::Config,
    pub cleanup_config: crate::engine::CleanupConfig,
    pub db_path: String,
}

pub enum Insecure {
    Deny,
    Allow,
}

pub fn validate(cfg: &Config, insecure: Insecure) -> Result<(), String> {
    match insecure {
        Insecure::Allow => {}
        Insecure::Deny => {
            if cfg
                .server_config
                .access_control
                .insecure_default_account
                .is_some()
            {
                return Err("insecure_default_account is not allowed in secure mode".to_owned());
            }
        }
    }
    Ok(())
}
