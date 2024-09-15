#[cfg(feature = "integration_tests")]
mod tests {
    use std::collections::HashSet;

    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
    use sea_orm_migration::MigratorTrait;
    use std::path::Path;

    use proglad_db as db;

    fn config(
        dir: impl AsRef<Path>,
        test_name: &str,
        db_path: &str,
    ) -> proglad_server::config::Config {
        let server_config = proglad_server::config::ServerConfig {
            port: 0, // TODO: pick unused port for testing
            site_base_url_path: "".to_owned(),
            auth_base_url: "".to_owned(),
            kratos_api_url: "".to_owned(),
            fs_root_dir: "".into(),
        };
        let manager_config = proglad_controller::manager::Config {
            container_name_prefix: format!("{test_name}-"),
            cache_dir: dir.as_ref().join("cache"),
            match_run_dir: dir.as_ref().join("matches"),
            template_dir: Default::default(),
            compilation_timeout: std::time::Duration::from_secs(30),
            agent_container_timeout: std::time::Duration::from_secs(3600),
            container_stdio_limit_bytes: 32000,
            match_dir_cleanup: None,
        };
        let match_runner_config = proglad_controller::match_runner::Config {
            send_timeout: std::time::Duration::from_nanos(10_000_000),
            sender_open_timeout: std::time::Duration::from_secs(3),
            player_ready_timeout: std::time::Duration::from_secs(3),
            kick_for_errors: true,
            max_player_errors: 10,
            line_length_limit: 1024,
        };
        let scheduler_config = proglad_server::scheduler::Config {
            enabled: true,
            compilation_check_period: Some(std::time::Duration::from_secs(1)),
            match_cleanup_check_period: Some(std::time::Duration::from_secs(5)),
        };
        let cleanup_config = proglad_server::engine::CleanupConfig {
            keep_matches_per_game: 5,
            max_delete_matches_num: 10,
        };
        proglad_server::config::Config {
            server_config,
            manager_config,
            match_runner_config,
            scheduler_config,
            cleanup_config,
            db_path: db_path.to_owned(),
        }
    }

    #[tokio::test]
    async fn server_smoke() {
        env_logger::Builder::from_env(env_logger::Env::default())
            .is_test(true)
            .filter_module("sqlx", log::LevelFilter::Error)
            .init();
        let dir = tempdir::TempDir::new("proglad-test").expect("Failed to create test dir");
        let path = dir.path();
        let test_name = path
            .file_name()
            .unwrap()
            .to_str()
            .expect("Failed to extract basename from test dir");
        let db_url = format!("sqlite://{}/db.sqlite?mode=rwc", path.to_str().unwrap());
        let db = sea_orm::Database::connect(&db_url)
            .await
            .expect("Failed to connect to the database");

        // TODO: populate the DB with simpler and faster games.
        // The default ones use go and rust that could be slower to compile.
        // Use C++ or python. Compilation of all languages should be tested separately.
        // Could be using faster simpler games as well.
        unsafe {
            std::env::set_var("PROGLAD_POPULATE_DATABASE", "true");
        }

        migration::Migrator::up(&db, None)
            .await
            .expect("Applying initial DB migrations failed");

        let bot_ids: Vec<i64> = db::bots::Entity::find()
            .select_only()
            .column(db::bots::Column::Id)
            .into_values::<i64, db::bots::Column>()
            .all(&db)
            .await
            .expect("Failed to get bots IDs from DB");

        let config = config(path, test_name, &db_url);
        tokio::fs::create_dir_all(&config.manager_config.cache_dir)
            .await
            .expect("Failed to create compilation cache dir");
        tokio::fs::create_dir_all(&config.manager_config.match_run_dir)
            .await
            .expect("Failed to create match run dir");
        let timeout = std::time::Duration::from_secs(120);
        log::info!("Running the server for {timeout:?}");
        let mut handle = proglad_server::server::create(config)
            .await
            .expect("Failed to create the server");
        let server_handle = handle.server.handle();
        let addrs = handle.addrs.clone();
        let server_join = tokio::task::spawn(async move {
            let _ = handle.server.await.inspect_err(|e| {
                log::error!("Running the server failed: {e:?}");
            });
        });
        tokio::time::sleep(timeout).await;
        handle.scheduler.cancel();
        handle
            .scheduler
            .join(std::time::Duration::from_secs(30))
            .await;

        // Check that database state is sane.
        let matches = db::matches::Entity::find()
            .all(&db)
            .await
            .expect("Failed to fetch matches from DB");
        let game1_completed_matches = matches
            .iter()
            .filter(|m| m.game_id == 1 && m.end_time.is_some())
            .collect::<Vec<_>>();
        let game2_completed_matches = matches
            .iter()
            .filter(|m| m.game_id == 2 && m.end_time.is_some())
            .collect::<Vec<_>>();
        assert!(!game1_completed_matches.is_empty());
        assert!(!game2_completed_matches.is_empty());
        let bot_stats = db::stats_history::Entity::find()
            .filter(db::stats_history::Column::Latest.eq(true))
            .all(&db)
            .await
            .expect("Failed to fetch bot stats from the DB");
        assert_eq!(
            bot_ids.iter().copied().collect::<HashSet<i64>>(),
            bot_stats
                .iter()
                .map(|bs| bs.bot_id)
                .collect::<HashSet<i64>>(),
            "The set of bots that got games and the full set of bots differ. This can be flaky."
        );

        let public_prog_id = db::programs::Entity::find()
            .filter(db::programs::Column::IsPublic.eq(true))
            .limit(1)
            .into_values::<i64, db::programs::Column>()
            .one(&db)
            .await
            .expect("Failed to fetch a public program from the db")
            .expect("No public programs in the DB");

        // Call some endpoints on the server itself.
        let addr = addrs.first().expect("No bound address found").to_string();
        let url_prefix = format!("http://{addr}/");
        let pages_to_test = [
            "".to_owned(),
            "games".to_owned(),
            "matches".to_owned(),
            "matches?game_id=1".to_owned(),
            "matches?game_id=1".to_owned(),
            "matches?account_id=1".to_owned(),
            "matches?game_id=1&account_id=1".to_owned(),
            "bots".to_owned(),
            "bots?game_id=1".to_owned(),
            "bots?game_id=2".to_owned(),
            "bots?account_id=1".to_owned(),
            "bots?game_id=1&account_id=1".to_owned(),
            "game/1".to_owned(),
            "game/2".to_owned(),
            "static/games/lowest-unique/index.html".to_owned(),
            "static/games/halma-quad/index.html".to_owned(),
            format!("visualizer/{}", game1_completed_matches[0].id),
            format!("visualizer/{}", game2_completed_matches[0].id),
            format!("replay/{}", game1_completed_matches[0].id),
            format!("replay/{}", game2_completed_matches[0].id),
            format!("source/{public_prog_id}"),
        ];
        for p in pages_to_test {
            reqwest::get(format!("{url_prefix}{p}"))
                .await
                .expect(&format!("failed to query page {p}"))
                .error_for_status()
                .expect(&format!("server returned an error for page {p}"));
        }

        server_handle.stop(true).await;
        let _ = server_join.await;
    }
}
