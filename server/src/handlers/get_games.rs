use crate::handlers::prelude::*;
use sea_orm::ConnectionTrait;

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

#[get("/games")]
pub async fn get_games(req: HttpRequest, session: Session) -> HttpResult {
    let state = server_state(&req)?;
    let requester = requester(&req, &session).await?;

    let games = db_allowed_games(&state.db, requester).await?;
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
                "{}/files/game/{}/icon.svg",
                state.config.site_base_url_path, g.id
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

async fn db_get_latest_match_with_replay_for_game(
    db: &DatabaseConnection,
    game_id: i64,
) -> Result<i64, DbErr> {
    // TODO: also make sure there is a replay.
    let match_id: Option<i64> = db::matches::Entity::find()
        .filter(sea_orm::Condition::all().add(db::matches::Column::GameId.eq(game_id)))
        .order_by_desc(db::matches::Column::EndTime)
        .select_only()
        .column(db::matches::Column::Id)
        .into_tuple()
        .one(db)
        .await?;
    match_id.ok_or_else(|| DbErr::RecordNotFound(format!("replay for game {game_id}")))
}

async fn db_allowed_games<C: ConnectionTrait>(db: &C, requester: Requester)
    -> Result<Vec<db::games::Model>, AppHttpError> {
    let all_games = db::prelude::Games::find()
        .order_by_asc(db::games::Column::Id)
        .all(db)
        .await
        .map_err(|e| {
            log::error!("Failed to select games from db: {e}");
            AppHttpError::Internal
        })?;
    let mut games = vec![];
    // TODO: batch acl check API.
    for g in all_games.into_iter() {
        match acl::check(db, requester, db::acls::AccessType::Read, db::common::EntityKind::Game, Some(g.id)).await {
            Ok(()) => games.push(g),
            Err(e) => {
                log::warn!("Filtered out game {}({}) because acl check failed: {e:?}", g.name, g.id);
            }
        }
    }
    Ok(games)
}
