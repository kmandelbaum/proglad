use actix_session::storage::CookieSessionStore;
use actix_session::SessionMiddleware;
use actix_web::{App, HttpServer};
use anyhow::Context;
use sea_orm::Database;

use std::sync::Arc;

use proglad_controller::manager;

use crate::config::*;
use crate::file_store::FileStore;
use crate::handlers;
use crate::scheduler;
use crate::server_state::*;

pub struct Handle {
    pub server: actix_web::dev::Server,
    pub scheduler: scheduler::Handle,
    pub addrs: Vec<std::net::SocketAddr>,
}

pub async fn run(config: Config) -> anyhow::Result<()> {
    let mut handle = create(config).await?;
    let result = handle.server.await;
    handle.scheduler.cancel();
    let timeout = std::time::Duration::from_secs(60);
    // This currently actually does not give matches time to complete, as the "docker"
    // processes that are spawned will also receive signals (e.g. SIGINT), being the
    // the same process group.
    // For now this is acceptable trade-off, but sometimes containers can leak.
    log::info!("Canceling background processes with timeout {timeout:?}.");
    handle.scheduler.join(timeout).await;
    result?;
    Ok(())
}

pub async fn create(config: Config) -> anyhow::Result<Handle> {
    let mut db_options = sea_orm::ConnectOptions::new(&config.db_path);
    db_options.max_connections(32);
    let db = Database::connect(db_options).await?;
    let man = Arc::new(manager::Manager::new(config.manager_config.clone()));
    let mut tmpl = handlebars::Handlebars::new();
    tmpl.set_strict_mode(true);
    tmpl.set_dev_mode(true);
    tmpl.register_templates_directory(
        config.server_config.fs_root_dir.join("templates"),
        Default::default(),
    )
    .context("Failed to register templates directory")?;
    let port = config.server_config.port;

    let file_store = FileStore {};
    let scheduler = scheduler::start(db.clone(), file_store.clone(), man, &config).await;
    let app_state = ServerState {
        tmpl,
        file_store,
        config: config.server_config,
        db,
    };

    let secret_key = actix_web::cookie::Key::generate();
    let server = HttpServer::new(move || {
        App::new()
            .wrap(SessionMiddleware::new(
                CookieSessionStore::default(),
                secret_key.clone(),
            ))
            .app_data(app_state.clone())
            .service(handlers::get_bots::get_bots)
            .service(handlers::get_edit_game::get_edit_game)
            .service(handlers::get_files::get_files)
            .service(handlers::get_files::get_files_nameless)
            .service(handlers::get_game::get_game)
            .service(handlers::get_games::get_games)
            .service(handlers::get_index::get_index)
            .service(handlers::get_logout::get_logout)
            .service(handlers::get_matches::get_matches)
            .service(handlers::get_visualizer::get_visualizer)
            .service(handlers::kratos_hooks::post_kratos_after_registration_hook)
            .service(handlers::kratos_hooks::post_kratos_after_settings_hook)
            .service(handlers::post_create_bot::post_create_bot)
            .service(handlers::post_edit_bot::post_edit_bot)
            .service(handlers::post_edit_game::post_edit_game)
            .service(handlers::post_schedule_match::post_schedule_match)
            .service(actix_files::Files::new(
                "/static",
                std::path::Path::new(&app_state.config.fs_root_dir).join("static"),
            ))
    })
    .workers(8)
    .bind(("::", port))?;
    let addrs = server.addrs();
    let server = server.run(); // Does not actually run the server but creates a future.
    Ok(Handle {
        server,
        scheduler,
        addrs,
    })
}
