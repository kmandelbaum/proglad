use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use proglad_controller::manager;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub enabled: bool,
    pub compilation_check_period: Option<std::time::Duration>,
    pub match_cleanup_check_period: Option<std::time::Duration>,
}

pub async fn start(
    db: DatabaseConnection,
    man: Arc<manager::Manager>,
    config: &crate::config::Config,
) {
    if !config.scheduler_config.enabled {
        log::info!("Scheduler is disabled.");
        return;
    }
    if let Some(compilation_period) = config.scheduler_config.compilation_check_period {
        let (db, man) = (db.clone(), man.clone());
        tokio::task::spawn(async move {
            loop {
                if !crate::engine::choose_and_compile_program(&db, &man).await {
                    tokio::time::sleep(compilation_period).await;
                }
            }
        });
    }
    if let Some(cleanup_period) = config.scheduler_config.match_cleanup_check_period {
        let db = db.clone();
        let cfg = config.cleanup_config.clone();
        tokio::task::spawn(async move {
            loop {
                tokio::time::sleep(cleanup_period).await;
                let _ = crate::engine::cleanup_matches_batch(&db, &cfg)
                    .await
                    .inspect_err(|e| log::error!("{e:?}"));
            }
        });
    }
    {
        let cfg = config.match_runner_config.clone();
        let man = man.clone();
        tokio::task::spawn(async move {
            loop {
                let _ = crate::engine::choose_and_run_match(&db, man.clone(), &cfg)
                    .await
                    .inspect_err(|e| log::error!("{e:?}"));
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });
    }

    if let Some(manager::MatchDirCleanup { period, .. }) =
        config.manager_config.match_dir_cleanup.as_ref()
    {
        let period = *period;
        let man = man.clone();
        tokio::task::spawn(async move {
            loop {
                let _ = man.cleanup_matches_iteration().await.inspect_err(|e| {
                    log::error!("Match dir cleanup failed: {e:?}");
                });
                tokio::time::sleep(period).await;
            }
        });
    }
}
