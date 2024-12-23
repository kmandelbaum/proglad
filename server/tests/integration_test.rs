#[cfg(feature = "integration_tests")]
mod tests {

    use std::collections::HashSet;
    use std::sync::Once;

    use sea_orm::{
        ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult, QueryFilter, QuerySelect,
    };
    use sea_orm_migration::MigratorTrait;
    use std::path::Path;

    use proglad_db as db;

    #[derive(FromQueryResult)]
    struct IdResult {
        id: i64,
    }

    static INIT: Once = Once::new();

    pub fn initialize() {
        INIT.call_once(|| {
            env_logger::Builder::from_env(env_logger::Env::default())
                .is_test(true)
                .filter_module("sqlx", log::LevelFilter::Error)
                .init();
        });
    }

    fn config(
        dir: impl AsRef<Path>,
        test_name: &str,
        db_path: &str,
    ) -> proglad_server::config::Config {
        let access_control = proglad_server::config::AccessControl {
            insecure_default_account: Some("km".to_owned()),
        };
        let server_config = proglad_server::config::ServerConfig {
            port: 0, // pick an unused port
            site_base_url_path: "".to_owned(),
            auth_base_url: "".to_owned(),
            kratos_api_url: "".to_owned(),
            fs_root_dir: "".into(),
            access_control,
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
            run_matches: true,
            scheduler_run_period: Some(std::time::Duration::from_millis(500)),
            match_cleanup_check_period: Some(std::time::Duration::from_secs(2)),
            max_scheduled_work_items: 5,
            match_run_default_priority: 1000,
            compilation_default_priority: 1500,
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

    struct Test {
        #[allow(dead_code)] // RAII for the temp dir
        dir: tempdir::TempDir,
        db: DatabaseConnection,
        config: proglad_server::config::Config,
    }
    async fn default_test_setup() -> Test {
        initialize();
        let dir = tempdir::TempDir::new("proglad-test").expect("Failed to create test dir");
        let path = dir.path().to_owned();
        let test_name = path
            .file_name()
            .unwrap()
            .to_str()
            .expect("Failed to extract basename from test dir")
            .to_owned();
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
        let config = config(&path, &test_name, &db_url);
        tokio::fs::create_dir_all(&config.manager_config.cache_dir)
            .await
            .expect("Failed to create compilation cache dir");
        tokio::fs::create_dir_all(&config.manager_config.match_run_dir)
            .await
            .expect("Failed to create match run dir");
        Test { dir, db, config }
    }

    #[tokio::test]
    async fn server_smoke() {
        let t = default_test_setup().await;
        let bot_ids: Vec<i64> = db::bots::Entity::find()
            .select_only()
            .column(db::bots::Column::Id)
            .into_values::<i64, db::bots::Column>()
            .all(&t.db)
            .await
            .expect("Failed to get bots IDs from DB");

        assert!(
            bot_ids.len() >= 3,
            "Expected at least 3 bots in default setup, found: {}",
            bot_ids.len()
        );
        let timeout = std::time::Duration::from_secs(120);
        log::info!("Running the server for {timeout:?}");
        let mut handle = proglad_server::server::create(t.config)
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
            .all(&t.db)
            .await
            .expect("Failed to fetch matches from DB");
        assert!(
            matches.len() >= 10,
            "Too few matches found: {}",
            matches.len()
        );
        assert!(
            matches.len() <= 13,
            "Too many matches found: {}, cleanup is probably not working",
            matches.len()
        );
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
            .all(&t.db)
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

        // Check that we have replays for all matches.
        let replay_match_ids = db::files::Entity::find()
            .filter(db::files::Column::OwningEntity.eq(db::common::EntityKind::Match))
            .select_only()
            .column_as(db::files::Column::OwningId, "id")
            .into_model::<IdResult>()
            .all(&t.db)
            .await
            .expect("Failed to fetch all file ids for match replays");
        assert_eq!(
            HashSet::<i64>::from_iter(replay_match_ids.iter().map(|i| i.id)),
            HashSet::<i64>::from_iter(matches.iter().map(|m| m.id))
        );

        let public_prog_id = db::programs::Entity::find()
            .filter(db::programs::Column::IsPublic.eq(true))
            .limit(1)
            .into_values::<i64, db::programs::Column>()
            .one(&t.db)
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
            "files/game/1/index.html".to_owned(),
            "files/game/2/index.html".to_owned(),
            "edit_game".to_owned(),
            format!("visualizer/{}", game1_completed_matches[0].id),
            format!("visualizer/{}", game2_completed_matches[0].id),
            format!("files/match/{}", game1_completed_matches[0].id),
            format!("files/match/{}", game2_completed_matches[0].id),
            format!("files/program/{public_prog_id}"),
        ];
        let client = reqwest::ClientBuilder::new()
            .gzip(true)
            .build()
            .expect("Failed to build reqwest client");
        for p in pages_to_test {
            client
                .get(format!("{url_prefix}{p}"))
                .send()
                .await
                .expect(&format!("failed to query page {p}"))
                .error_for_status()
                .expect(&format!("server returned an error for page {p}"));
        }

        server_handle.stop(true).await;
        let _ = server_join.await;
    }

    #[tokio::test]
    async fn create_bot() {
        let t = default_test_setup().await;
        // Cleanup some bots and games, leaving one bot and one game to compile.
        let game = db::games::Entity::find()
            .filter(db::games::Column::Name.eq("lowest-unique"))
            .one(&t.db)
            .await
            .expect("Failed to get game id for lowest-unique")
            .expect("Game lowest-unique not found");
        let game_id = game.id;
        let bots = db::bots::Entity::find()
            .filter(db::bots::Column::GameId.eq(game_id))
            .all(&t.db)
            .await
            .expect("Failed to fetch bots");
        // Leave only 2 bots for lowest-unique.
        let bots = bots[..2].to_vec();
        db::bots::Entity::delete_many()
            .filter(db::bots::Column::Id.is_not_in(bots.iter().map(|b| b.id)))
            .exec(&t.db)
            .await
            .expect("Failed to delete extraneous bots");
        db::games::Entity::delete_many()
            .filter(db::games::Column::Name.ne("lowest-unique"))
            .exec(&t.db)
            .await
            .expect("Failed to delete extraneous games");
        db::programs::Entity::delete_many()
            .filter(
                db::programs::Column::Id.is_not_in(
                    bots.iter()
                        .map(|b| b.program_id)
                        .chain(std::iter::once(game.program_id)),
                ),
            )
            .exec(&t.db)
            .await
            .expect("Failed to delete extraneous programs");

        let mut handle = proglad_server::server::create(t.config)
            .await
            .expect("Failed to create the server");
        let server_handle = handle.server.handle();
        let addrs = handle.addrs.clone();
        let server_join = tokio::task::spawn(async move {
            let _ = handle.server.await.inspect_err(|e| {
                log::error!("Running the server failed: {e:?}");
            });
        });
        let addr = addrs.first().expect("No bound address found").to_string();
        let url_prefix = format!("http://{addr}/");

        let client = reqwest::ClientBuilder::new()
            .gzip(true)
            .build()
            .expect("Failed to build reqwest client");
        let source_code = tokio::fs::read("../games/lowest-unique/player-random/main.py")
            .await
            .expect("Failed to read example source file");
        let source_file = reqwest::multipart::Part::bytes(source_code.clone());
        let form = reqwest::multipart::Form::new()
            .text("language", "python")
            .text("name", "test-bot-1")
            .part("file", source_file);

        let resp = client
            .post(format!("{url_prefix}create_bot/{game_id}"))
            .multipart(form)
            .send()
            .await
            .expect("Failed to send create bot request")
            .error_for_status()
            .expect("Create bot request failed");
        let body = resp
            .text()
            .await
            .expect("Failed to get body after bot creation request.");
        assert!(body.contains("test-bot-1"), "{body}");

        let new_bot = db::bots::Entity::find()
            .filter(db::bots::Column::Id.is_not_in(bots.iter().map(|b| b.id)))
            .one(&t.db)
            .await
            .expect("Failed to get new bot from db")
            .expect("New bot not found in the db");

        db::acls::set_program_public(&t.db, new_bot.program_id, true)
            .await
            .expect("Failed to set bot to public");

        let resp = client
            .get(format!("{url_prefix}files/program/{}", new_bot.program_id))
            .send()
            .await
            .expect(&format!(
                "failed to query source code for new bot {:?}",
                new_bot
            ))
            .error_for_status()
            .expect(&format!(
                "server returned an error when asked for source of the new program"
            ));
        assert_eq!(
            resp.text()
                .await
                .expect("failed to get text from the new program response")
                .as_bytes(),
            source_code
        );

        // Should be enough to run some matches.
        let timeout = std::time::Duration::from_secs(45);
        log::info!("Running the server for {timeout:?}");
        tokio::time::sleep(timeout).await;
        handle.scheduler.cancel();
        handle
            .scheduler
            .join(std::time::Duration::from_secs(30))
            .await;
        let matches = db::matches::Entity::find()
            .all(&t.db)
            .await
            .expect("Failed to fetch matches from DB");
        assert_ne!(matches.len(), 0);
        server_handle.stop(true).await;
        let _ = server_join.await;
    }

    #[tokio::test]
    async fn create_game() {
        let t = default_test_setup().await;
        db::bots::Entity::delete_many()
            .exec(&t.db)
            .await
            .expect("Failed to delete extraneous bots");
        db::games::Entity::delete_many()
            .exec(&t.db)
            .await
            .expect("Failed to delete extraneous games");
        db::programs::Entity::delete_many()
            .exec(&t.db)
            .await
            .expect("Failed to delete extraneous programs");

        let mut handle = proglad_server::server::create(t.config)
            .await
            .expect("Failed to create the server");
        let server_handle = handle.server.handle();
        let addrs = handle.addrs.clone();
        let server_join = tokio::task::spawn(async move {
            let _ = handle.server.await.inspect_err(|e| {
                log::error!("Running the server failed: {e:?}");
            });
        });
        let addr = addrs.first().expect("No bound address found").to_string();
        let url_prefix = format!("http://{addr}/");

        let source_code = tokio::fs::read("../games/lowest-unique/server/main.rs")
            .await
            .expect("Failed to read gameserver source code");
        let gameserver_file =
            reqwest::multipart::Part::bytes(source_code).file_name("weirdname.rs");
        let markdown_file = reqwest::multipart::Part::bytes("**Bold Test Game**".as_bytes())
            .file_name("weirdname.md");
        let icon_file =
            reqwest::multipart::Part::bytes("<svg></svg>".as_bytes()).file_name("weirdname.svg");

        let client = reqwest::ClientBuilder::new()
            .gzip(true)
            .build()
            .expect("Failed to build reqwest client");

        let form = reqwest::multipart::Form::new()
            .text("game_name", "test-game")
            .text("description", "Simple Game For Testing")
            .text("min_players", "3")
            .text("max_players", "6")
            .text("language", "rust")
            .text("param_string", "{num_players} 10 500 inlinevisualize")
            .part("markdown_file", markdown_file)
            .part("icon_file", icon_file)
            .part("gameserver_file", gameserver_file);
        let resp = client
            .post(format!("{url_prefix}edit_game"))
            .multipart(form)
            .send()
            .await
            .expect("Failed to send edit game request")
            .error_for_status()
            .expect("Edit game request failed");
        let _ = resp
            .text()
            .await
            .expect("Failed to get body after game creation request.");

        let games = db::games::Entity::find()
            .all(&t.db)
            .await
            .expect("Failed to get games from the db");
        assert_eq!(games.len(), 1);
        let game_id = games[0].id;

        let source_code = tokio::fs::read("../games/lowest-unique/player-random/main.py")
            .await
            .expect("Failed to read example source file");
        let create_bot = |name| {
            let source_code = &source_code;
            let client = &client;
            let url_prefix = &url_prefix;
            async move {
                let source_file = reqwest::multipart::Part::bytes(source_code.clone());
                let form = reqwest::multipart::Form::new()
                    .text("language", "python")
                    .text("name", name)
                    .part("file", source_file);
                let resp = client
                    .post(format!("{url_prefix}create_bot/{game_id}"))
                    .multipart(form)
                    .send()
                    .await
                    .expect("Failed to send create bot request")
                    .error_for_status()
                    .expect("Create bot request failed");
                let body = resp
                    .text()
                    .await
                    .expect("Failed to get body after bot creation request.");
                assert!(body.contains(name), "{body}");
            }
        };
        create_bot("test-bot-1").await;
        create_bot("test-bot-2").await;
        create_bot("test-bot-3").await;

        // Should be enough to compile.
        let timeout = std::time::Duration::from_secs(10);
        tokio::time::sleep(timeout).await;

        let _ = client
            .post(format!("{url_prefix}schedule_match/{game_id}"))
            .send()
            .await
            .expect("Failed to send schedule match request")
            .error_for_status()
            .expect("Create bot request failed");

        // Should be enough to run a match.
        let timeout = std::time::Duration::from_secs(10);
        tokio::time::sleep(timeout).await;

        let matches = db::matches::Entity::find()
            .all(&t.db)
            .await
            .expect("Failed to fetch matches from DB");
        assert!(!matches.is_empty(), "No matches were played");
        let match_id = matches[0].id;
        let body = client
            .get(format!("{url_prefix}files/match/{match_id}"))
            .send()
            .await
            .expect("Failed to get match replay")
            .error_for_status()
            .expect("Match replay request return error status")
            .text()
            .await
            .expect("Failed to get match replay text");
        assert!(body.contains(" over "), "Match ran but was not successful. Replay:\n{body}");
        assert!(body.contains(" vis "), "No visualizer commands in the replay:\n{body}");

        handle.scheduler.cancel();
        server_handle.stop(true).await;
        let _ = server_join.await;
    }
}
