use actix_multipart::form::{tempfile::TempFile, MultipartForm};
use sea_orm::{prelude::TimeDateTimeWithTimeZone, ConnectionTrait, Set};

use crate::handlers::prelude::*;
use crate::validation::*;

#[derive(Debug, MultipartForm)]
struct EditGameForm {
    #[multipart(limit = "64KB")]
    gameserver_file: TempFile,
    #[multipart(limit = "64KB")]
    markdown_file: TempFile,
    #[multipart(limit = "128KB")]
    icon_file: TempFile,
    #[multipart(limit = "1KB")]
    game_name: actix_multipart::form::text::Text<String>,
    #[multipart(limit = "1KB")]
    description: actix_multipart::form::text::Text<String>,
    #[multipart(limit = "1KB")]
    language: actix_multipart::form::text::Text<String>,
    #[multipart(limit = "1KB")]
    max_players: actix_multipart::form::text::Text<i32>,
    #[multipart(limit = "1KB")]
    min_players: actix_multipart::form::text::Text<i32>,
    #[multipart(limit = "1KB")]
    param_string: actix_multipart::form::text::Text<String>,
}

#[derive(Deserialize)]
struct EditGameQuery {
    game_id: Option<i64>,
}

#[derive(Serialize)]
struct GameRulesTmplData<'a> {
    base_url_path: &'a str,
    body_html: String,
}

#[post("/edit_game")]
async fn post_edit_game(
    req: HttpRequest,
    session: Session,
    q: web::Query<EditGameQuery>,
    MultipartForm(form): MultipartForm<EditGameForm>,
) -> impl Responder {
    // TODO: set multipart error handler
    // TODO: csrf tokens throughout
    let state = server_state(&req)?;
    let mut validation_errors = vec![];
    let requester = requester(&req, &session).await?;

    let name = match validate_game_name(form.game_name.as_str()) {
        Ok(_) => Some(form.game_name.as_str().to_owned()),
        Err(e) => return Err(AppHttpError::GameNameValidationFailed(e)),
    };

    let language = parse_language(&form.language)?;

    let min_players = match validate_players_number(*form.min_players) {
        Ok(_) => Some(*form.min_players),
        Err(e) => {
            validation_errors.push(format!("min_players: {e}"));
            Some(1)
        }
    };
    let mut max_players = match validate_players_number(*form.max_players) {
        Ok(_) => Some(*form.max_players),
        Err(e) => {
            validation_errors.push(format!("max_players: {e}"));
            Some(1)
        }
    };
    if max_players.is_some() && min_players.is_some() && max_players < min_players {
        max_players = min_players;
    }
    let gameserver_source = if form.gameserver_file.size != 0 {
        Some(
            tokio::fs::read(form.gameserver_file.file.path())
                .await
                .map_err(|e| {
                    log::error!(
                        "Failed to read temp source file {:?}: {e}",
                        form.gameserver_file.file.path()
                    );
                    AppHttpError::Internal
                })?,
        )
    } else {
        None
    };
    let mut update = db::games::ActiveModel::default();
    if let Some(name) = name {
        update.name = Set(name);
    }
    if let Some(mp) = min_players {
        update.min_players = Set(mp);
    }
    if let Some(mp) = max_players {
        update.max_players = Set(mp);
    }
    update.description = Set(form.description.to_string());
    update.param = Set(Some(form.param_string.as_str().to_owned()));
    let file_store = state.file_store.clone();
    let game_id = state
        .db
        .transaction(|txn| {
            Box::pin(async move {
                let now = time::OffsetDateTime::now_utc();
                let game = match q.game_id {
                    None => None,
                    Some(game_id) => {
                        let game = db::games::Entity::find_by_id(game_id)
                            .one(txn)
                            .await
                            .map_err(|e| {
                                log::error!("Failed to fetch game id {game_id}: {e}");
                                AppHttpError::Internal
                            })?;
                        let Some(game) = game else {
                            log::error!("Game {game_id} not found");
                            return Err(AppHttpError::NotFound);
                        };
                        update.id = Set(game.id);
                        Some(game)
                    }
                };
                let game_id = match &game {
                    None => {
                        let program = db::programs::ActiveModel {
                            language: Set(language),
                            status: Set(db::programs::Status::New),
                            status_update_time: Set(now),
                            ..Default::default()
                        };
                        let program_id = db::programs::Entity::insert(program)
                            .exec(txn)
                            .await
                            .map_err(|e| {
                                log::error!("Failed to insert program: {e}");
                                AppHttpError::Internal
                            })?
                            .last_insert_id;
                        update.program_id = Set(program_id);
                        update.status = Set(db::games::Status::InDevelopment);
                        let game_id = db::games::Entity::insert(update)
                            .exec(txn)
                            .await
                            .map_err(|e| {
                                log::error!("Failed to insert new game: {e}");
                                if let Some(sea_orm::error::SqlErr::UniqueConstraintViolation(s)) =
                                    e.sql_err()
                                {
                                    AppHttpError::GameNameAlreadyTaken(s)
                                } else {
                                    AppHttpError::Internal
                                }
                            })?
                            .last_insert_id;
                        add_rw_acl(txn, requester, db::common::EntityKind::Program, program_id)
                            .await?;
                        add_rw_acl(txn, requester, db::common::EntityKind::Game, game_id).await?;
                        if let Some(source) = gameserver_source {
                            write_source(&file_store, txn, requester, program_id, source)
                                .await
                                .map_err(file_error_to_http_error)?;
                        }
                        game_id
                    }
                    Some(g) => {
                        acl::check(
                            txn,
                            requester,
                            db::acls::AccessType::Write,
                            db::common::EntityKind::Game,
                            Some(g.id),
                        )
                        .await
                        .map_err(acl_check_to_http_error)?;
                        let mut program_update = db::programs::ActiveModel {
                            id: Set(g.program_id),
                            language: Set(language),
                            ..Default::default()
                        };
                        let program = db::programs::Entity::find_by_id(g.program_id)
                            .one(txn)
                            .await
                            .map_err(|e| {
                                log::error!(
                                    "Failed to fetch program {} for game {}: {e:?}",
                                    g.program_id,
                                    g.id
                                );
                                AppHttpError::Internal
                            })?
                            .ok_or_else(|| {
                                log::error!("Program {} for game {} not found", g.program_id, g.id);
                                AppHttpError::Internal
                            })?;
                        if program.language != language || gameserver_source.is_some() {
                            program_update.status_update_time =
                                Set(TimeDateTimeWithTimeZone::now_utc());
                            program_update.status = Set(db::programs::Status::New);
                            db::programs::Entity::update(program_update)
                                .exec(txn)
                                .await
                                .map_err(|e| {
                                    log::error!("Failed to update program {}: {e}", g.program_id);
                                    AppHttpError::Internal
                                })?;
                        }
                        update.program_id = Set(g.program_id);
                        db::games::Entity::update(update)
                            .exec(txn)
                            .await
                            .map_err(|e| {
                                log::error!("Failed to update game {}: {e}", g.id);
                                if let Some(sea_orm::error::SqlErr::UniqueConstraintViolation(s)) =
                                    e.sql_err()
                                {
                                    AppHttpError::GameNameAlreadyTaken(s)
                                } else {
                                    AppHttpError::Internal
                                }
                            })?;
                        if let Some(source) = gameserver_source {
                            write_source(&file_store, txn, requester, g.program_id, source)
                                .await
                                .map_err(file_error_to_http_error)?;
                        }
                        g.id
                    }
                };
                Ok(game_id)
            })
        })
        .await
        .map_err(|e| match e {
            sea_orm::TransactionError::Connection(_) => AppHttpError::Internal,
            sea_orm::TransactionError::Transaction(e) => e,
        })?;

    if form.markdown_file.size > 0 {
        if let Ok(markdown_content) = tokio::fs::read_to_string(form.markdown_file.file.path())
            .await
            .inspect_err(|e| {
                log::error!("Failed to read temp file in the form: {e}");
                validation_errors.push(format!("{e:?}"));
            })
        {
            if let Ok(rules_html) =
                markdown::to_html_with_options(&markdown_content, &markdown::Options::gfm())
                    .inspect_err(|e| {
                        log::error!("Failed to convert markdown: {e}");
                        validation_errors.push(format!("{e:?}"));
                    })
            {
                let full_html = state
                    .tmpl
                    .render(
                        "game_rules",
                        &GameRulesTmplData {
                            base_url_path: &state.config.site_base_url_path,
                            body_html: rules_html,
                        },
                    )
                    .map_err(|e| {
                        log::error!("Failed to render game_rules: {e}");
                        AppHttpError::Internal
                    })?;
                write_content(
                    &state.file_store,
                    &state.db,
                    requester,
                    game_id,
                    "index.html".to_owned(),
                    db::files::ContentType::Html,
                    full_html.into_bytes(),
                )
                .await
                .map_err(file_error_to_http_error)?;
            }
        }
    }

    if form.icon_file.size > 0 {
        let ext = form
            .icon_file
            .file_name
            .as_ref()
            .and_then(|n| <String as AsRef<std::path::Path>>::as_ref(n).extension())
            .and_then(|e| e.to_str());
        let (content_type, extension) = match ext {
            //Some("png") => (db::files::ContentType::Png, "png"),
            Some("svg") => (db::files::ContentType::Svg, "svg"),
            Some(s) => return Err(AppHttpError::UnsupportedImageType(s.to_owned())),
            None => return Err(AppHttpError::UnsupportedImageType("".to_owned())),
        };
        if let Ok(icon_content) = tokio::fs::read(form.icon_file.file.path())
            .await
            .inspect_err(|e| {
                log::error!("Failed to read temp file in the form: {e}");
                validation_errors.push(format!("{e:?}"));
            })
        {
            write_content(
                &state.file_store,
                &state.db,
                requester,
                game_id,
                format!("icon.{extension}"),
                content_type,
                icon_content,
            )
            .await
            .map_err(file_error_to_http_error)?;
        }
    }
    Ok::<_, AppHttpError>(
        web::Redirect::to(format!(
            "{}/edit_game?game_id={game_id}",
            state.config.site_base_url_path
        ))
        .see_other()
        .respond_to(&req),
    )
}

async fn write_source<C: ConnectionTrait>(
    file_store: &FileStore,
    db: &C,
    requester: Requester,
    program_id: i64,
    content: Vec<u8>,
) -> Result<(), file_store::Error> {
    let file = db::files::Model {
        owning_entity: db::common::EntityKind::Program,
        owning_id: Some(program_id),
        content_type: db::files::ContentType::PlainText,
        kind: db::files::Kind::SourceCode,
        content: Some(content),
        last_update: TimeDateTimeWithTimeZone::now_utc(),
        name: String::new(),
        compression: db::files::Compression::Uncompressed,
        ..Default::default()
    };
    let file = FileStore::compress(file)?;
    file_store.write(db, requester, file).await
}

async fn write_content<C: ConnectionTrait>(
    file_store: &FileStore,
    db: &C,
    requester: Requester,
    game_id: i64,
    name: String,
    content_type: db::files::ContentType,
    content: Vec<u8>,
) -> Result<(), file_store::Error> {
    let file = db::files::Model {
        name,
        owning_entity: db::common::EntityKind::Game,
        owning_id: Some(game_id),
        content_type,
        kind: db::files::Kind::StaticContent,
        content: Some(content),
        last_update: TimeDateTimeWithTimeZone::now_utc(),
        compression: db::files::Compression::Uncompressed,
        ..Default::default()
    };
    let file = if content_type != db::files::ContentType::Png {
        FileStore::compress(file)?
    } else {
        file
    };
    file_store.write(db, requester, file).await
}

async fn add_rw_acl<C: ConnectionTrait>(
    db: &C,
    requester: Requester,
    entity_kind: db::common::EntityKind,
    entity_id: i64,
) -> Result<(), AppHttpError> {
    let Requester::Account(owner_id) = requester else {
        return Err(AppHttpError::Unauthenticated);
    };
    let base = db::acls::ActiveModel {
        grantee_kind: Set(db::acls::GranteeKind::Account),
        grantee_id: Set(Some(owner_id)),
        entity_kind: Set(entity_kind),
        entity_id: Set(Some(entity_id)),
        ..Default::default()
    };
    let mut access_types = vec![db::acls::AccessType::Read, db::acls::AccessType::Write];
    if entity_kind == db::common::EntityKind::Game {
        access_types.push(db::acls::AccessType::ReadMatchesOfGame);
        access_types.push(db::acls::AccessType::CreateBotsInGame);
    }
    let updates = access_types
        .into_iter()
        .map(|access_type| db::acls::ActiveModel {
            access_type: Set(access_type),
            ..base.clone()
        });

    db::acls::Entity::insert_many(updates)
        .exec(db)
        .await
        .map_err(|e| {
            log::error!("Failed to insert acls for {entity_kind:?} {entity_id}: {e:?}");
            AppHttpError::Internal
        })?;
    Ok(())
}
