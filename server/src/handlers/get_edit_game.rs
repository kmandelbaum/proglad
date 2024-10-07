use crate::handlers::prelude::*;
use sea_orm::{Condition, ConnectionTrait};

#[derive(Serialize)]
struct EditGameTmplData<'a> {
    base_url_path: &'a str,
    title: String,
    game_id: i64,
    game_name: String,
    description: String,
    min_players: i32,
    max_players: i32,
    param: String,
    languages: Vec<LanguageChoice>,
    bots: Vec<BotOnEditGamePageTmplData>,
    program: Option<ProgramTmplData>,
    matches: Option<MatchesTmplData>,
}

#[derive(Serialize)]
struct ProgramTmplData {
    id: i64,
    language: String,
    updated: String,
    status: String,
}

#[derive(Serialize)]
struct BotOnEditGamePageTmplData {
    bot_id: i64,
    name: String,
    status: String,
}

#[derive(Serialize)]
struct MatchOnEditGamePageTmplData {
    match_id: Option<i64>,
    status: String,
    update_time: String,
}

#[derive(Serialize)]
struct MatchesTmplData {
    unstartable_reason: Option<String>,
    matches: Vec<MatchOnEditGamePageTmplData>,
}

#[derive(Deserialize)]
struct EditGameQuery {
    game_id: Option<i64>,
}

#[get("/edit_game")]
pub async fn get_edit_game(
    req: HttpRequest,
    session: Session,
    q: web::Query<EditGameQuery>,
) -> HttpResult {
    let state = server_state(&req)?;
    let game = if let Some(game_id) = q.game_id {
        let requester = requester(&req, &session).await?;
        acl::check(
            &state.db,
            requester,
            db::acls::AccessType::Read,
            db::common::EntityKind::Game,
            Some(game_id),
        )
        .await
        .map_err(acl_check_to_http_error)?;
        let game = db::games::Entity::find_by_id(game_id)
            .one(&state.db)
            .await
            .map_err(|e| {
                log::error!("get_edit_game: Failed to fetch game {game_id}: {e}.");
                AppHttpError::Internal
            })?;
        if game.is_none() {
            log::error!("get_edit_game: Game {game_id} not found");
            return Err(AppHttpError::NotFound);
        }
        game
    } else {
        None
    };
    let st = server_state(&req)?;
    let program = match &game {
        None => None,
        Some(g) => db::programs::Entity::find_by_id(g.program_id)
            .one(&state.db)
            .await
            .map_err(|e| {
                log::error!("Faled to get program for game {}: {e}", g.id);
                AppHttpError::Internal
            })?,
    };
    let language = program.as_ref().map(|p| p.language);
    let data = match game {
        None => EditGameTmplData {
            base_url_path: &state.config.site_base_url_path,
            title: "Create Game".to_owned(),
            game_id: 0,
            game_name: "".to_owned(),
            description: "".to_owned(),
            min_players: 1,
            max_players: 1,
            param: "".to_owned(),
            languages: language_choices(None),
            bots: vec![],
            program: None,
            matches: None,
        },
        Some(g) => {
            let bots_and_programs = db::bots::Entity::find()
                .filter(db::bots::Column::GameId.eq(g.id))
                .find_also_related(db::programs::Entity)
                .all(&state.db)
                .await
                .map_err(|e| {
                    log::error!("Failed to fetch bots for game: {e}");
                    AppHttpError::Internal
                })?;
            let match_status = check_match_readiness(&g, program.as_ref(), &bots_and_programs);
            let bots = bots_and_programs
                .into_iter()
                .map(|(b, p)| BotOnEditGamePageTmplData {
                    bot_id: b.id,
                    name: b.name.clone(),
                    status: bot_status(&b, p.as_ref()),
                })
                .collect::<Vec<_>>();
            let program = program.map(|p| ProgramTmplData {
                id: p.id,
                language: p.language.as_str().to_owned(),
                status: format!("{:?}", p.status),
                updated: format_time(p.status_update_time),
            });
            let scheduled_matches = db_get_scheduled_matches(&state.db, g.id, Limit(5))
                .await
                .map_err(|e| {
                    log::error!("Failed to get scheduled matches for game {}: {e:?}", g.id);
                    AppHttpError::Internal
                })?;
            let completed_matches =
                db_get_matches(&state.db, g.id, Limit(5))
                    .await
                    .map_err(|e| {
                        log::error!("Failed to get completed matches for game {}: {e:?}", g.id);
                        AppHttpError::Internal
                    })?;
            let matches = scheduled_matches
                .into_iter()
                .map(scheduled_match_to_tmpl_data)
                .chain(completed_matches.into_iter().map(match_to_tmpl_data))
                .collect();
            EditGameTmplData {
                base_url_path: &state.config.site_base_url_path,
                title: "Edit Game".to_owned(),
                game_id: g.id,
                game_name: g.name,
                description: g.description,
                min_players: g.min_players,
                max_players: g.max_players,
                param: g.param.unwrap_or_default(),
                languages: language_choices(language),
                bots,
                program,
                matches: Some(MatchesTmplData {
                    unstartable_reason: match_status.err(),
                    matches,
                }),
            }
        }
    };
    let html = st.tmpl.render("edit_game", &data).map_err(|e| {
        log::error!("Failed to render edit_game: {e}");
        AppHttpError::Internal
    })?;
    Ok(HttpResponse::Ok()
        .append_header(ContentType(mime::TEXT_HTML))
        .body(html))
}

fn check_match_readiness(
    game: &db::games::Model,
    program: Option<&db::programs::Model>,
    bots_and_programs: &[(db::bots::Model, Option<db::programs::Model>)],
) -> Result<(), String> {
    let program = program.ok_or_else(|| "No game server program".to_owned())?;
    let good_bot_count = bots_and_programs
        .iter()
        .filter(|(b, _)| bot_is_ready(b))
        .count();
    if program.status != db::programs::Status::CompilationSucceeded {
        return Err(format!("Program status: {:?}", program.status));
    }
    if (good_bot_count as i32) < game.min_players {
        return Err(format!(
            "Not enough ready bots: {}, min players: {}",
            good_bot_count, game.min_players
        ));
    }
    Ok(())
}

fn bot_is_ready(bot: &db::bots::Model) -> bool {
    bot.system_status == db::bots::SystemStatus::Ok
        && bot.owner_set_status == db::bots::OwnerSetStatus::Active
}

struct Limit(u64);

async fn db_get_matches<C: ConnectionTrait>(
    db: &C,
    game_id: i64,
    Limit(limit): Limit,
) -> Result<Vec<db::matches::Model>, DbErr> {
    db::matches::Entity::find()
        .filter(db::matches::Column::GameId.eq(game_id))
        .order_by_desc(db::matches::Column::CreationTime)
        .limit(limit)
        .all(db)
        .await
}

async fn db_get_scheduled_matches<C: ConnectionTrait>(
    db: &C,
    game_id: i64,
    Limit(limit): Limit,
) -> Result<Vec<db::work_items::Model>, DbErr> {
    db::work_items::Entity::find()
        .filter(
            Condition::all()
                .add(db::work_items::Column::Status.eq(db::work_items::Status::Scheduled))
                .add(db::work_items::Column::WorkType.eq(db::work_items::WorkType::RunMatch))
                .add(db::work_items::Column::GameId.eq(Some(game_id))),
        )
        .order_by_desc(db::work_items::Column::CreationTime)
        .limit(limit)
        .all(db)
        .await
}

fn match_to_tmpl_data(m: db::matches::Model) -> MatchOnEditGamePageTmplData {
    MatchOnEditGamePageTmplData {
        update_time: format_time(m.last_update_time()),
        match_id: Some(m.id),
        status: m.system_message,
    }
}

fn scheduled_match_to_tmpl_data(w: db::work_items::Model) -> MatchOnEditGamePageTmplData {
    MatchOnEditGamePageTmplData {
        match_id: None,
        update_time: format_time(w.last_update_time()),
        status: "Scheduled".to_owned(),
    }
}
