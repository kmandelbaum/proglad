use crate::handlers::prelude::*;

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
pub async fn get_bots(
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
