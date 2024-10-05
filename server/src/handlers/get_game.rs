use crate::handlers::prelude::*;

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

#[derive(Serialize, Clone, Debug)]
struct BriefMatchTmplData {
    match_id: i64,
    system_message: String,
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
    let bots = db_active_bots_of_game(&state.db, game_id)
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
    let reference_bot_languages = db_languages_of_programs(
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

async fn db_active_bots_of_game(
    db: &DatabaseConnection,
    game_id: i64,
) -> Result<Vec<db::bots::Model>, DbErr> {
    let condition = sea_orm::Condition::all()
        .add(db::bots::Column::GameId.eq(game_id))
        .add(db::bots::Column::SystemStatus.eq(db::bots::SystemStatus::Ok))
        .add(db::bots::Column::OwnerSetStatus.eq(db::bots::OwnerSetStatus::Active));
    db::bots::Entity::find().filter(condition).all(db).await
}

async fn db_languages_of_programs(
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
