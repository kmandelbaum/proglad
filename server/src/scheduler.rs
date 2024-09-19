use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::oneshot;

use proglad_controller::manager;

#[derive(Default)]
pub struct Handle {
    cancel_senders: Vec<oneshot::Sender<()>>,
    join_handles: Vec<tokio::task::JoinHandle<()>>,
}

impl Handle {
    pub fn cancel(&mut self) {
        for sender in std::mem::take(&mut self.cancel_senders) {
            let _ = sender.send(());
        }
    }
    pub async fn join(self, timeout: std::time::Duration) {
        let mut deadline = tokio::time::interval_at(
            tokio::time::Instant::now() + timeout, timeout);
        let mut join_set = tokio::task::JoinSet::from_iter(self.join_handles);
        loop {
            tokio::select! {
                _ = deadline.tick() => break,
                next_result = join_set.join_next() => if next_result.is_none() { break; }
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub enabled: bool,
    pub scheduler_run_period: Option<std::time::Duration>,
    pub match_cleanup_check_period: Option<std::time::Duration>,
    pub max_scheduled_work_items: usize,
    pub match_run_default_priority: i64,
    pub compilation_default_priority: i64,
}

pub async fn start(
    db: DatabaseConnection,
    man: Arc<manager::Manager>,
    config: &crate::config::Config,
) -> Handle {
    let mut handle = Handle::default();
    if !config.scheduler_config.enabled {
        log::info!("Scheduler is disabled.");
        return handle;
    }
    {
        let man = man.clone();
        let db = db.clone();
        let cfg = config.match_runner_config.clone();
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        let j = tokio::task::spawn(async move {
            loop {
                let _ = crate::engine::select_and_run_work_item(&db, man.clone(), &cfg)
                    .await
                    .inspect_err(|e| log::error!("{e:?}"));
                // Give the server a bit of a break.
                // TODO: remove sleep.
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                    Ok(()) = &mut cancel_rx => break
                }
            }
            log::info!("Match runner loop canceled.");
        });
        handle.cancel_senders.push(cancel_tx);
        handle.join_handles.push(j);
    }
    if let Some(period) = config.scheduler_config.scheduler_run_period {
        let db = db.clone();
        let cfg = config.scheduler_config.clone();
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        let j = tokio::task::spawn(async move {
            loop {
                let _ = crate::engine::scheduling_round(&db, &cfg)
                    .await
                    .inspect_err(|e| log::error!("{e:?}"));
                tokio::select! {
                    _ = tokio::time::sleep(period) => {}
                    Ok(()) = &mut cancel_rx => break
                }
            }
            log::info!("Match runner loop canceled.");
        });
        handle.cancel_senders.push(cancel_tx);
        handle.join_handles.push(j);
    }
    if let Some(cleanup_period) = config.scheduler_config.match_cleanup_check_period {
        let db = db.clone();
        let cfg = config.cleanup_config.clone();
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        let j = tokio::task::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(cleanup_period) => {}
                    Ok(()) = &mut cancel_rx => break
                }
                let _ = crate::engine::cleanup_matches_batch(&db, &cfg)
                    .await
                    .inspect_err(|e| log::error!("{e:?}"));
            }
            log::info!("Match cleanup loop canceled.");
        });
        handle.cancel_senders.push(cancel_tx);
        handle.join_handles.push(j);
    }
    if let Some(manager::MatchDirCleanup { period, .. }) =
        config.manager_config.match_dir_cleanup.as_ref()
    {
        let period = *period;
        let man = man.clone();
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        let j = tokio::task::spawn(async move {
            loop {
                let _ = man.cleanup_matches_iteration().await.inspect_err(|e| {
                    log::error!("Match dir cleanup failed: {e:?}");
                });
                tokio::select! {
                    _ = tokio::time::sleep(period) => {}
                    Ok(()) = &mut cancel_rx => break
                }
            }
            log::info!("Match dir cleanup loop canceled.");
        });
        handle.join_handles.push(j);
        handle.cancel_senders.push(cancel_tx);
    }
    handle
}
