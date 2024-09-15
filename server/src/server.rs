use actix_multipart::form::{tempfile::TempFile, MultipartForm};
use actix_session::storage::CookieSessionStore;
use actix_session::{Session, SessionMiddleware};
use actix_web::http::header::ContentType;
use actix_web::{get, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use anyhow::Context;
use sea_orm::{
    ColumnTrait, Database, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, TransactionTrait,
};
use sea_query::IntoCondition;
use serde::{Deserialize, Serialize};

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::Arc;

use proglad_controller::manager;
use proglad_db as db;

use crate::bot::*;
use crate::config::*;
use crate::engine;
use crate::http_types::*;
use crate::kratos::{self, *};
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
    let tf = |t: &str| -> std::path::PathBuf {
        std::path::Path::new(&config.server_config.fs_root_dir)
            .join("templates")
            .join(format!("{t}.hbs"))
    };
    tmpl.register_template_file("creatematch", tf("creatematch"))
        .context("Failed to register creatematch template")?;
    tmpl.register_template_file("games", tf("games"))
        .context("Failed to register games template")?;
    tmpl.register_template_file("visualizer", tf("visualizer"))
        .context("Failed to register visualizer template")?;
    tmpl.register_template_file("game", tf("game"))
        .context("Failed to register game template")?;
    tmpl.register_template_file("bots", tf("bots"))
        .context("Failed to register bots template")?;
    tmpl.register_template_file("matches", tf("matches"))
        .context("Failed to register matches")?;
    tmpl.register_template_file("main", tf("main"))
        .context("Failed to register main template")?;
    let port = config.server_config.port;

    let scheduler = scheduler::start(db.clone(), man, &config).await;
    let app_state = ServerState {
        tmpl,
        db,
        config: config.server_config,
    };

    let secret_key = actix_web::cookie::Key::generate();
    let server = HttpServer::new(move || {
        App::new()
            .wrap(SessionMiddleware::new(
                CookieSessionStore::default(),
                secret_key.clone(),
            ))
            .app_data(app_state.clone())
            .service(get_games)
            .service(get_game)
            .service(post_create_bot)
            .service(get_match)
            .service(main_page)
            .service(get_logout)
            .service(post_kratos_after_registration_hook)
            .service(post_kratos_after_settings_hook)
            .service(get_replay)
            .service(get_visualizer)
            .service(get_bots)
            .service(get_matches)
            .service(get_source)
            .service(actix_files::Files::new(
                "/static",
                std::path::Path::new(&app_state.config.fs_root_dir).join("static"),
            ))
    })
    .workers(8)
    .bind(("::", port))?;
    let addrs = server.addrs();
    let server = server.run(); // Does not actually run the server but creates a future.
    Ok(Handle { server, scheduler, addrs })
}

#[derive(Serialize)]
struct MainPageTmplData<'a> {
    base_url_path: &'a str,
    auth_url: &'a str,
    authenticated: bool,
    account_id: i64,
}

#[get("/")]
async fn main_page(req: HttpRequest, session: Session) -> HttpResult {
    let state = server_state(&req)?;
    let config = &state.config;
    let account_id = kratos_authenticate(&req, &session).await?;
    let html = state
        .tmpl
        .render(
            "main",
            &MainPageTmplData {
                base_url_path: &config.site_base_url_path,
                auth_url: &config.auth_base_url,
                authenticated: account_id.is_some(),
                account_id: account_id.unwrap_or_default(),
            },
        )
        .map_err(|e| {
            log::error!("Failed to render main page template: {e}");
            AppHttpError::Internal
        })?;
    Ok(HttpResponse::Ok()
        .append_header(ContentType(mime::TEXT_HTML))
        .body(html))
}

#[derive(Deserialize)]
struct LogoutInfo {
    finished: Option<bool>,
}

#[get("/logout")]
async fn get_logout(
    req: HttpRequest,
    session: Session,
    info: web::Query<LogoutInfo>,
) -> impl Responder {
    session.purge();
    if info.finished == Some(true) {
        return Ok::<_, AppHttpError>(
            web::Redirect::to(format!(
                "{}/",
                server_state(&req)?.config.site_base_url_path
            ))
            .see_other()
            .respond_to(&req),
        );
    }
    kratos_logout(&req).await
}

#[post("/kratos_after_registration_hook")]
async fn post_kratos_after_registration_hook(
    req: HttpRequest,
    info: web::Json<kratos::AccountInfo>,
) -> HttpResult {
    kratos_after_registrtation_hook(req, info).await
}

#[post("/kratos_after_settings_hook")]
async fn post_kratos_after_settings_hook(
    req: HttpRequest,
    info: web::Json<kratos::AccountInfo>,
) -> HttpResult {
    // Intentionally the same as after registration.
    kratos_after_registrtation_hook(req, info).await
}

#[derive(Serialize, Clone, Debug)]
struct ParticipationTmplData {
    ingame_player: u32,
    bot_name: String,
    score: String,
    highlight: bool,
    system_message: String,
}

#[derive(Serialize, Clone, Debug)]
struct MatchTmplData {
    match_id: i64,
    // TODO: use three stages here - creted, started, finished and show that time instead.
    creation_time: String,
    game_id: i64,
    game_name: String,
    participations: Vec<ParticipationTmplData>,
    duration: String,
    system_message: String,
}

#[derive(Serialize, Clone, Debug)]
struct BriefMatchTmplData {
    match_id: i64,
    system_message: String,
}

#[derive(Serialize, Clone, Debug)]
struct MatchesTmplData<'a> {
    base_url_path: &'a str,
    matches: Vec<MatchTmplData>,
}

#[derive(Deserialize, Debug)]
struct FilterInfo {
    account_id: Option<i64>,
    game_id: Option<i64>,
}

#[get("/matches")]
async fn get_matches(req: HttpRequest, info: web::Query<FilterInfo>) -> HttpResult {
    let state = server_state(&req)?;
    let maybe_filter_by_game = match info.game_id {
        None => sea_orm::Condition::all(),
        Some(game_id) => sea_orm::Condition::all().add(db::matches::Column::GameId.eq(game_id)),
    };
    let (matches, owner_bots) = match info.account_id {
        Some(account_id) => {
            // TODO: figure out how to filter out the bots for an account with too many bots.
            // One option is to store the last match participation timestamp on each bot.
            let owner_bots = db_bots_of_account(&state.db, account_id)
                .await
                .map_err(|e| {
                    log::error!("Failed to fetch bots of account {account_id}: {e:?}");
                    AppHttpError::Internal
                })?;
            // TODO: here we can filter the recent ones.
            let matches = db_matches_of_bots(
                &state.db,
                owner_bots.iter().map(|b| b.id),
                maybe_filter_by_game,
                100,
            )
            .await
            .map_err(|e| {
                log::error!("Failed to fetch matches of owner {account_id}: {e:?}");
                AppHttpError::Internal
            })?;
            let owner_bots = HashSet::<i64>::from_iter(owner_bots.into_iter().map(|b| b.id));
            (matches, owner_bots)
        }
        None => {
            let matches = db_recent_matches(&state.db, maybe_filter_by_game, 100)
                .await
                .map_err(|e| {
                    log::error!("Failed to fetch recent matches: {e}");
                    AppHttpError::Internal
                })?;
            let owner_bots = HashSet::<i64>::new();
            (matches, owner_bots)
        }
    };
    let base_url_path = state.config.site_base_url_path.clone();
    let matches_data =
        match_tmpl_data(&state.db, &matches, |p| owner_bots.contains(&p.bot_id)).await?;
    let html = state
        .tmpl
        .render(
            "matches",
            &MatchesTmplData {
                base_url_path: &base_url_path,
                matches: matches_data,
            },
        )
        .map_err(|e| {
            log::error!("Failed to render bots template: {e}");
            AppHttpError::Internal
        })?;
    Ok(HttpResponse::Ok()
        .append_header(ContentType(mime::TEXT_HTML))
        .body(html))
}

#[get("/bots_of_game/{game_id}")]
async fn get_bots_of_game(
    req: HttpRequest,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppHttpError> {
    let state = server_state(&req)?;
    let bots = db_bots_of_game(&state.db, *path, BotQueryMode::All)
        .await
        .map_err(|e| {
            log::error!("Failed to fetch bots of game {}: {e:?}", *path);
            AppHttpError::Internal
        })?;
    todo!("{:?}", bots)
}

#[derive(Clone, Serialize)]
struct BotRowTmplData {
    name: String,
    game: String,
    owner: String,
    language: String,
    created: String,
    status: String,
    updated: String,
}

#[derive(Clone, Serialize)]
struct BotsTmplData<'a> {
    base_url_path: &'a str,
    bots: Vec<BotRowTmplData>,
    show_owner: bool,
}

#[get("/bots")]
async fn get_bots(
    req: HttpRequest,
    info: web::Query<FilterInfo>,
) -> Result<HttpResponse, AppHttpError> {
    let state = server_state(&req)?;
    let base_url_path = &state.config.site_base_url_path;
    const MAX_BOTS: u64 = 1000;
    let mut filter = sea_orm::Condition::all();
    if let Some(game_id) = info.game_id {
        filter = filter.add(db::bots::Column::GameId.eq(game_id))
    }
    if let Some(account_id) = info.account_id {
        filter = filter.add(db::bots::Column::OwnerId.eq(account_id))
    }
    let bots = db::bots::Entity::find()
        .filter(filter)
        .order_by_desc(db::bots::Column::StatusUpdateTime)
        .limit(MAX_BOTS)
        .all(&state.db)
        .await
        .map_err(|e| {
            log::error!("Failed to get bots of {info:?} : {e:?}");
            AppHttpError::Internal
        })?;
    let users = bots.iter().map(|b| b.owner_id).collect::<HashSet<_>>();
    // TODO: cache usernames of accounts.
    let users = db_usernames(&state.db, users.into_iter())
        .await
        .map_err(|e| {
            log::error!("Failed to get accounts map of {info:?}: {e:?}");
            AppHttpError::Internal
        })?;
    let programs = db_programs_metadata(&state.db, bots.iter().map(|b| b.program_id))
        .await
        .map_err(|e| {
            log::error!("Failed to fetch programs of bots of {info:?}: {e:?}");
            AppHttpError::Internal
        })?;
    let games = db_games(
        &state.db,
        bots.iter()
            .map(|b| b.game_id)
            .collect::<HashSet<i64>>()
            .into_iter(),
    )
    .await
    .map_err(|e| {
        log::error!("Failed to fetch games of bots of {info:?}: {e:?}");
        AppHttpError::Internal
    })?;
    let games = games
        .into_iter()
        .map(|g| (g.id, g))
        .collect::<HashMap<_, _>>();
    let programs: HashMap<i64, db::programs::Model> =
        programs.into_iter().map(|p| (p.id, p)).collect();
    let bots = bots
        .into_iter()
        .map(|b| {
            let program = programs.get(&b.program_id);
            let game = games.get(&b.game_id);
            let status = format!(
                "{:?} | {}",
                b.system_status,
                program.map_or("No program".to_owned(), |p| format!("{:?}", p.status))
            );
            let updated = program.map_or(b.status_update_time, |p| {
                p.status_update_time.max(b.status_update_time)
            });
            BotRowTmplData {
                name: b.name,
                game: game.map_or(String::new(), |g| g.name.clone()),
                language: program.map_or(String::new(), |p| format!("{:?}", p.language)),
                created: format_time(b.creation_time),
                updated: format_time(updated),
                status,
                owner: users.get(&b.owner_id).cloned().unwrap_or_default(),
            }
        })
        .collect();
    let html = state
        .tmpl
        .render(
            "bots",
            &BotsTmplData {
                base_url_path,
                bots,
                show_owner: info.account_id.is_none(),
            },
        )
        .map_err(|e| {
            log::error!("Failed to render bots template: {e}");
            AppHttpError::Internal
        })?;
    Ok(HttpResponse::Ok()
        .append_header(ContentType(mime::TEXT_HTML))
        .body(html))
}

#[get("/match/{id}")]
async fn get_match(_req: HttpRequest, path: web::Path<i64>) -> impl Responder {
    format!("Getting match id {}", *path)
}

#[derive(Serialize)]
struct GamesTmplData<'a> {
    base_url_path: &'a str,
    games: Vec<GameCardTmplData>,
}

#[derive(Serialize, Clone)]
struct GameCardTmplData {
    name: String,
    title: String,
    description: String,
    num_bots: usize,
    num_authors: usize,
    image_src: String,
    url: String,
    replay_url: String,
}

#[derive(Serialize, Clone)]
struct BotOnGamePageTmplData {
    bot_id: i64,
    owner: String,
    name: String,
    matches_played: usize,
    average_score: String,
}

#[derive(Serialize, Clone)]
struct ReferenceBotTmplData {
    language: String,
    source_url: String,
}

#[derive(Serialize, Clone)]
struct GameTmplData<'a> {
    base_url_path: &'a str,
    game_id: i64,
    title: String,
    url: String,
    bots: Vec<BotOnGamePageTmplData>,
    active_bots_num: usize,
    reference_bots: Vec<ReferenceBotTmplData>,
    matches: Vec<BriefMatchTmplData>,
}

#[get("/games")]
async fn get_games(req: HttpRequest) -> HttpResult {
    let state = server_state(&req)?;

    let games: Vec<db::games::Model> = db::prelude::Games::find()
        .order_by_asc(db::games::Column::Id)
        .all(&state.db)
        .await
        .map_err(|e| {
            log::error!("Failed to select games from db: {e}");
            AppHttpError::Internal
        })?;
    let bot_game_ids: Vec<(i64, i64)> = db::prelude::Bots::find()
        .select_only()
        .column(db::bots::Column::GameId)
        .column(db::bots::Column::OwnerId)
        .into_tuple()
        .all(&state.db)
        .await
        .map_err(|e| {
            log::error!("Failed to select games from db: {e}");
            AppHttpError::Internal
        })?;
    let mut bot_counts = HashMap::<i64, usize>::new();
    let mut authors = HashMap::<i64, HashSet<i64>>::new();
    for (game_id, owner_id) in bot_game_ids {
        *bot_counts.entry(game_id).or_default() += 1;
        authors.entry(game_id).or_default().insert(owner_id);
    }
    let mut match_ids = Vec::with_capacity(games.len());
    for g in games.iter() {
        match_ids.push(
            db_get_latest_match_with_replay_for_game(&state.db, g.id)
                .await
                .inspect_err(|e| {
                    log::error!("Failed to fetch latest replay for game {}: {e}", g.id)
                })
                .ok(),
        );
    }

    let games: Vec<GameCardTmplData> = games
        .into_iter()
        .zip(match_ids.into_iter())
        .map(|(g, latest_match_id)| GameCardTmplData {
            name: g.name.clone(),
            title: g.name.clone(),
            description: g.description,
            image_src: format!(
                "{}/static/games/{}/icon.svg",
                state.config.site_base_url_path, g.name
            ),
            url: format!("{}/game/{}", state.config.site_base_url_path, g.id),
            num_bots: bot_counts.get(&g.id).cloned().unwrap_or_default(),
            num_authors: authors.get(&g.id).map_or(0, |s| s.len()),
            replay_url: latest_match_id.map_or("".to_owned(), |i| {
                format!("{}/visualizer/{i}", state.config.site_base_url_path)
            }),
        })
        .collect();
    let html = state
        .tmpl
        .render(
            "games",
            &GamesTmplData {
                base_url_path: &state.config.site_base_url_path,
                games,
            },
        )
        .map_err(|e| {
            log::error!("Failed to render games template: {e}");
            AppHttpError::Internal
        })?;
    Ok(HttpResponse::Ok()
        .append_header(ContentType(mime::TEXT_HTML))
        .body(html))
}

#[get("/game/{game_id}")]
async fn get_game(req: HttpRequest, path: web::Path<i64>) -> HttpResult {
    let game_id = *path;
    let state = server_state(&req)?;
    // TODO: a custom select to fetch everything instead.
    let Some(game) = db::games::Entity::find_by_id(game_id)
        .one(&state.db)
        .await
        .map_err(|e| {
            log::error!("Failed to fetch game {game_id} from db: {e:?}");
            AppHttpError::Internal
        })?
    else {
        return Err(AppHttpError::NotFound);
    };
    let url = format!(
        "{}/static/games/{}/index.html",
        state.config.site_base_url_path, game.name
    );
    let bots = db_bots_of_game(&state.db, game_id, BotQueryMode::Active)
        .await
        .map_err(|e| {
            log::error!("Failed to fetch bots for game {game_id} from db: {e:?}");
            AppHttpError::Internal
        })?;
    let active_bots_num = bots.len();
    let usernames = db_usernames(&state.db, bots.iter().map(|b| b.owner_id))
        .await
        .map_err(|e| {
            log::error!("Failed to fetch bots for game {game_id} from db: {e:?}");
            AppHttpError::Internal
        })?;
    let reference_bot_languages = languages_of_programs(
        &state.db,
        bots.iter()
            .filter(|b| b.is_reference_bot.unwrap_or(false))
            .map(|b| b.program_id),
    )
    .await
    .map_err(|e| {
        log::error!(
            "Failed to fetch languages of programs for reference bots of game {game_id}: {e:?}"
        );
        AppHttpError::Internal
    })?;
    let reference_bots = bots
        .iter()
        .filter(|b| b.is_reference_bot.unwrap_or(false))
        .map(|b| {
            let language = reference_bot_languages
                .iter()
                .find(|(id, _)| *id == b.program_id)
                .map_or("Unknown Language".to_owned(), |(_, lang)| {
                    lang.as_str().to_owned()
                });
            ReferenceBotTmplData {
                language,
                source_url: format!(
                    "{}/source/{}",
                    state.config.site_base_url_path, b.program_id
                ),
            }
        })
        .collect::<Vec<_>>();
    const MAX_BOTS: usize = 10;
    const MAX_MATCHES: u64 = 10;
    let stats = db_bot_stats(&state.db, bots.iter().map(|b| b.id))
        .await
        .map_err(|e| {
            log::error!("Failed to fetch bots stats for game {game_id} from db: {e:?}");
            AppHttpError::Internal
        })?;
    let bots = HashMap::<i64, db::bots::Model>::from_iter(bots.into_iter().map(|b| (b.id, b)));
    let mut bot_scores = stats
        .iter()
        .map(|(i, s)| (average_score(s), i))
        .collect::<Vec<_>>();
    bot_scores.sort_by(|x, y| x.partial_cmp(y).unwrap().reverse());
    let bots = bot_scores
        .iter()
        .take(MAX_BOTS)
        .map(|(_, id)| {
            let b = bots.get(id).unwrap();
            let st = stats.get(id).unwrap();
            BotOnGamePageTmplData {
                bot_id: b.id,
                owner: usernames
                    .get(&b.owner_id)
                    .cloned()
                    .unwrap_or("unknown".to_owned()),
                name: b.name.clone(),
                matches_played: st.total_matches as usize,
                average_score: format!("{:.2}", average_score(st)),
            }
        })
        .collect::<Vec<_>>();
    let matches = db_recent_matches(
        &state.db,
        db::matches::Column::GameId.eq(game_id).into_condition(),
        MAX_MATCHES,
    )
    .await
    .map_err(|e| {
        log::error!("Failed to fetch recent matches for game {game_id}: {e:?}");
        AppHttpError::Internal
    })?;
    let matches = matches
        .into_iter()
        .map(|m| BriefMatchTmplData {
            match_id: m.id,
            system_message: m.system_message,
        })
        .collect::<Vec<_>>();
    let html = state
        .tmpl
        .render(
            "game",
            &GameTmplData {
                game_id: game.id,
                base_url_path: &state.config.site_base_url_path,
                title: game.name,
                url,
                active_bots_num,
                bots,
                reference_bots,
                matches,
            },
        )
        .map_err(|e| {
            log::error!("Failed to render 'game' template: {e:?}");
            AppHttpError::Internal
        })?;
    Ok(HttpResponse::Ok()
        .append_header(ContentType(mime::TEXT_HTML))
        .body(html))
}

#[get("/source/{id}")]
async fn get_source(req: HttpRequest, _session: Session, path: web::Path<i64>) -> HttpResult {
    // TODO: use session to authenticate and determin if the user has access.
    let program_id = *path;
    let program = db::programs::Entity::find_by_id(program_id)
        .one(&server_state(&req)?.db)
        .await
        .map_err(|e| {
            log::error!("Failed to get program id {program_id}: {e:?}");
            AppHttpError::Internal
        })?;
    let Some(program) = program else {
        return Err(AppHttpError::NotFound);
    };
    if !program.is_public.unwrap_or(false) {
        return Err(AppHttpError::Unauthorized);
    }
    Ok(HttpResponse::Ok()
        .append_header(ContentType(mime::TEXT_PLAIN_UTF_8))
        .body(program.source_code.unwrap_or_default()))
}

#[derive(Debug, MultipartForm)]
struct CreateBotForm {
    #[multipart(limit = "64KB")]
    file: TempFile,
    language: actix_multipart::form::text::Text<String>,
    name: actix_multipart::form::text::Text<String>,
}

#[post("/create_bot/{game_id}")]
async fn post_create_bot(
    MultipartForm(form): MultipartForm<CreateBotForm>,
    req: HttpRequest,
    session: Session,
    path: web::Path<i64>,
) -> impl Responder {
    let game_id = *path;
    let state = server_state(&req)?;
    let language = parse_language(&form.language)?;
    let Some(owner) = kratos_authenticate(&req, &session).await? else {
        return Err(AppHttpError::Unauthenticated);
    };
    if let Err(e) = validate_bot_name(&form.name) {
        return Err(AppHttpError::InvalidBotName { s: StringError(e) });
    }
    // TODO: move this thing into engine.
    let txn_result = state
        .db
        .transaction(|txn| {
            Box::pin(async move {
                let existing_bots = db::bots::Entity::find()
                    .filter(
                        sea_orm::Condition::all()
                            .add(db::bots::Column::OwnerId.eq(owner))
                            .add(db::bots::Column::Name.eq(form.name.as_str()))
                            .add(db::bots::Column::GameId.eq(game_id)),
                    )
                    .all(txn)
                    .await;
                match existing_bots {
                    Ok(bots) => {
                        if !bots.is_empty() {
                            return Err(AppHttpError::BotAlreadyExists);
                        }
                    }
                    Err(e) => {
                        log::error!("Error while checking for existing bots: {e}");
                    }
                }

                engine::create_bot(txn, game_id, owner, form.file.file, language, &form.name)
                    .await
                    .map_err(|e| {
                        log::info!("Failed to create bot for game {game_id}: {e:?}");
                        AppHttpError::Internal
                    })?;
                Ok(())
            })
        })
        .await;
    match txn_result {
        Err(sea_orm::TransactionError::Connection(e)) => {
            log::info!("Creating bot failed due to TransactionError::Connection({e:?})");
            Err(AppHttpError::Internal)
        }
        Err(sea_orm::TransactionError::Transaction(e)) => Err(e),
        Ok(()) => Ok::<_, AppHttpError>(
            web::Redirect::to(format!("{}/bots", state.config.site_base_url_path))
                .see_other()
                .respond_to(&req),
        ),
    }
}

#[get("/replay/{match_id}")]
async fn get_replay(req: HttpRequest, path: web::Path<i64>) -> HttpResult {
    let state = server_state(&req)?;
    let replay = db_get_replay(&state.db, *path).await.map_err(|e| {
        log::error!("Failed to get replay: {e}");
        AppHttpError::BadClientData
    })?;
    Ok(HttpResponse::Ok().body(replay))
}

#[derive(Serialize)]
struct VisualizerTmplData<'a> {
    base_url_path: &'a str,
    match_id: i64,
    match_data: MatchTmplData,
}

#[get("/visualizer/{match_id}")]
async fn get_visualizer(req: HttpRequest, path: web::Path<i64>) -> HttpResult {
    let state = server_state(&req)?;
    db_check_match_exists_and_has_replay(&state.db, *path).await?;
    let matches = db::matches::Entity::find_by_id(*path)
        .all(&state.db)
        .await
        .map_err(|e| {
            log::error!("Failed to get match {}: {e:?}", *path);
            AppHttpError::NotFound
        })?;
    let match_data = match_tmpl_data(&state.db, &matches, |_| false).await?;
    if match_data.len() != 1 {
        log::error!(
            "Failed to construct match template data: found {} match tmpl data, expected 1.",
            match_data.len()
        );
        return Err(AppHttpError::Internal);
    }
    let html = state
        .tmpl
        .render(
            "visualizer",
            &VisualizerTmplData {
                base_url_path: &state.config.site_base_url_path,
                match_id: *path,
                match_data: match_data.into_iter().next().unwrap(),
            },
        )
        .map_err(|e| {
            log::error!("Failed to render template: {e}");
            AppHttpError::Internal
        })?;
    Ok(HttpResponse::Ok()
        .append_header(ContentType(mime::TEXT_HTML))
        .body(html))
}

async fn db_get_replay(db: &DatabaseConnection, match_id: i64) -> Result<String, DbErr> {
    let Some(log) = db::matches::Entity::find_by_id(match_id)
        .one(db)
        .await?
        .and_then(|m| m.log)
    else {
        return Err(DbErr::RecordNotFound(format!("{match_id}")));
    };

    let mut gz = flate2::read::GzDecoder::new(log.as_slice());
    let mut s = String::new();
    gz.read_to_string(&mut s)
        .map_err(|e| DbErr::Custom(format!("{e}")))?;
    Ok(s)
}

async fn db_get_latest_match_with_replay_for_game(
    db: &DatabaseConnection,
    game_id: i64,
) -> Result<i64, DbErr> {
    let match_id: Option<i64> = db::matches::Entity::find()
        .filter(
            sea_orm::Condition::all()
                .add(db::matches::Column::GameId.eq(game_id))
                .add(db::matches::Column::Log.is_not_null()),
        )
        .order_by_desc(db::matches::Column::EndTime)
        .select_only()
        .column(db::matches::Column::Id)
        .into_tuple()
        .one(db)
        .await?;
    match_id.ok_or_else(|| DbErr::RecordNotFound(format!("replay for game {game_id}")))
}

async fn db_check_match_exists_and_has_replay(
    db: &DatabaseConnection,
    match_id: i64,
) -> Result<(), AppHttpError> {
    let count: Option<(i32,)> = db::matches::Entity::find_by_id(match_id)
        .filter(db::matches::Column::Log.is_not_null().into_condition())
        .select_only()
        .expr(db::matches::Column::Id.count())
        .into_tuple()
        .one(db)
        .await
        .map_err(|e| {
            log::error!("Match {match_id} does not exist or checking failed: {e}");
            AppHttpError::NotFound
        })?;
    if count.map_or(true, |(x,)| x == 0) {
        Err(AppHttpError::NotFound)
    } else {
        Ok(())
    }
}

async fn db_bot_owners_and_names(
    db: &DatabaseConnection,
    ids: impl IntoIterator<Item = i64>,
) -> Result<Vec<(i64, String, String)>, DbErr> {
    db::bots::Entity::find()
        .left_join(db::accounts::Entity)
        .filter(db::bots::Column::Id.is_in(ids))
        .select_only()
        .column(db::bots::Column::Id)
        .column(db::accounts::Column::Name)
        .column(db::bots::Column::Name)
        .into_tuple()
        .all(db)
        .await
}

enum BotQueryMode {
    All,
    Active,
}

async fn db_bots_of_game(
    db: &DatabaseConnection,
    game_id: i64,
    mode: BotQueryMode,
) -> Result<Vec<db::bots::Model>, DbErr> {
    let condition = sea_orm::Condition::all().add(db::bots::Column::GameId.eq(game_id));
    let condition = match mode {
        BotQueryMode::All => condition,
        BotQueryMode::Active => condition
            .add(db::bots::Column::SystemStatus.eq(db::bots::SystemStatus::Ok))
            .add(db::bots::Column::OwnerSetStatus.eq(db::bots::OwnerSetStatus::Active)),
        //BotQueryMode::HavingUnknownStatus => condition
        //    .add(db::bots::Column::SystemStatus.eq(db::bots::SystemStatus::Unknown))
        //    .add(db::bots::Column::OwnerSetStatus.eq(db::bots::OwnerStatus::Active)),
    };
    db::bots::Entity::find().filter(condition).all(db).await
}

async fn languages_of_programs(
    db: &DatabaseConnection,
    ids: impl IntoIterator<Item = i64>,
) -> Result<Vec<(i64, db::programs::Language)>, DbErr> {
    db::programs::Entity::find()
        .select_only()
        .column(db::programs::Column::Id)
        .column(db::programs::Column::Language)
        .filter(db::programs::Column::Id.is_in(ids))
        .into_tuple()
        .all(db)
        .await
}

async fn db_usernames(
    db: &DatabaseConnection,
    ids: impl Iterator<Item = i64>,
) -> Result<HashMap<i64, String>, DbErr> {
    Ok(db::accounts::Entity::find()
        .filter(db::accounts::Column::Id.is_in(ids))
        .all(db)
        .await?
        .into_iter()
        .map(|acc| (acc.id, acc.name))
        .collect())
}

async fn db_bots_of_account(
    db: &DatabaseConnection,
    account_id: i64,
) -> Result<Vec<db::bots::Model>, DbErr> {
    db::bots::Entity::find()
        .filter(db::bots::Column::OwnerId.eq(account_id))
        .order_by_desc(db::bots::Column::CreationTime)
        .all(db)
        .await
}

async fn db_matches_of_bots(
    db: &DatabaseConnection,
    bot_ids: impl IntoIterator<Item = i64>,
    match_filter: sea_orm::Condition,
    limit: u64,
) -> Result<Vec<db::matches::Model>, DbErr> {
    let participations_and_matches = db::match_participations::Entity::find()
        .filter(db::match_participations::Column::BotId.is_in(bot_ids))
        .find_also_related(db::matches::Entity)
        .filter(match_filter)
        .order_by_desc(db::matches::Column::EndTime)
        .limit(limit)
        .all(db)
        .await?;
    Ok(participations_and_matches
        .into_iter()
        .filter_map(|(_, m)| m)
        .collect())
}

async fn db_recent_matches(
    db: &DatabaseConnection,
    filter: sea_orm::Condition,
    limit: u64,
) -> Result<Vec<db::matches::Model>, DbErr> {
    db::matches::Entity::find()
        .filter(filter)
        .order_by_desc(db::matches::Column::EndTime)
        .limit(limit)
        .all(db)
        .await
}

async fn db_participations_in_matches(
    db: &DatabaseConnection,
    match_ids: impl IntoIterator<Item = i64>,
) -> Result<Vec<db::match_participations::Model>, DbErr> {
    db::match_participations::Entity::find()
        .filter(db::match_participations::Column::MatchId.is_in(match_ids))
        .all(db)
        .await
}

async fn db_games(
    db: &DatabaseConnection,
    ids: impl Iterator<Item = i64>,
) -> Result<Vec<db::games::Model>, DbErr> {
    db::games::Entity::find()
        .filter(db::games::Column::Id.is_in(ids))
        .all(db)
        .await
}

async fn db_programs_metadata(
    db: &DatabaseConnection,
    ids: impl Iterator<Item = i64>,
) -> Result<Vec<db::programs::Model>, DbErr> {
    db::programs::Entity::find()
        .filter(db::programs::Column::Id.is_in(ids))
        .select_only()
        .columns({
            use db::programs::Column::*;
            [Id, Language, Status, StatusReason, StatusUpdateTime]
        })
        .all(db)
        .await
}

fn average_score(st: &db::stats_history::Model) -> f64 {
    st.total_score / (st.total_matches.max(1) as f64)
}

async fn db_bot_stats(
    db: &DatabaseConnection,
    ids: impl ExactSizeIterator<Item = i64>,
) -> Result<HashMap<i64, db::stats_history::Model>, DbErr> {
    let stats = db::stats_history::Entity::find()
        .filter(
            sea_orm::Condition::all()
                .add(db::stats_history::Column::Latest.eq(true))
                .add(db::stats_history::Column::BotId.is_in(ids)),
        )
        .all(db)
        .await?;
    Ok(stats.into_iter().map(|st| (st.bot_id, st)).collect())
}

async fn match_tmpl_data(
    db: &DatabaseConnection,
    matches: &[db::matches::Model],
    highlight: impl Fn(&db::match_participations::Model) -> bool,
) -> Result<Vec<MatchTmplData>, AppHttpError> {
    let games = db_games(
        db,
        matches
            .iter()
            .map(|m| m.game_id)
            .collect::<HashSet<_>>()
            .into_iter(),
    )
    .await
    .map_err(|e| {
        log::error!("Failed to fetch game names: {e:?}");
        AppHttpError::Internal
    })?;
    let participations = db_participations_in_matches(db, matches.iter().map(|m| m.id))
        .await
        .map_err(|e| {
            log::error!("Failed to fetch match participations for matches of owner: {e:?}");
            AppHttpError::Internal
        })?;
    let bot_owners_and_names = db_bot_owners_and_names(
        db,
        participations
            .iter()
            .map(|p| p.bot_id)
            .collect::<HashSet<_>>(),
    )
    .await
    .map_err(|e| {
        log::error!("Failed to fetch all participating bots: {e:?}");
        AppHttpError::Internal
    })?;
    let bot_names = HashMap::<i64, String>::from_iter(
        bot_owners_and_names
            .into_iter()
            .map(|(id, owner, name)| (id, format!("{owner}/{name}"))),
    );
    let mut matches_data = HashMap::<i64, MatchTmplData>::new();
    let game_names = HashMap::<i64, String>::from_iter(games.into_iter().map(|g| (g.id, g.name)));
    for m in matches {
        let duration = m
            .end_time
            .and_then(|end| m.start_time.map(|start| format_duration(end - start)))
            .unwrap_or_default();
        matches_data.insert(
            m.id,
            MatchTmplData {
                match_id: m.id,
                creation_time: format_time(m.creation_time),
                game_id: m.game_id,
                game_name: game_names.get(&m.game_id).cloned().unwrap_or_default(),
                participations: vec![],
                duration,
                system_message: m.system_message.clone(),
            },
        );
    }
    for p in participations.into_iter() {
        let Some(md) = matches_data.get_mut(&p.match_id) else {
            continue;
        };
        md.participations.push(ParticipationTmplData {
            ingame_player: p.ingame_player,
            bot_name: bot_names.get(&p.bot_id).cloned().unwrap_or_default(),
            highlight: highlight(&p),
            system_message: p.system_message.unwrap_or_default(),
            score: p.score.map_or(String::new(), |s| format!("{s:.2}")),
        });
    }
    for m in matches_data.values_mut() {
        m.participations.sort_by_key(|p| p.ingame_player);
    }
    let mut matches_data = matches_data.into_values().collect::<Vec<_>>();
    matches_data.sort_by(|md1, md2| md1.creation_time.cmp(&md2.creation_time).reverse());
    Ok(matches_data)
}

fn parse_language(language: &str) -> Result<db::programs::Language, AppHttpError> {
    Ok(match language {
        "cpp" => db::programs::Language::Cpp,
        "go" => db::programs::Language::Go,
        "java" => db::programs::Language::Java,
        "python" => db::programs::Language::Python,
        "rust" => db::programs::Language::Rust,
        _ => return Err(AppHttpError::BadClientData),
    })
}

fn format_time(time: time::OffsetDateTime) -> String {
    let format = time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    time.format(&format).unwrap()
}

fn format_duration(duration: time::Duration) -> String {
    format!("{:.3}s", duration.as_seconds_f32())
}
