use crate::handlers::prelude::*;

#[derive(Serialize, Clone, Debug)]
struct MatchesTmplData<'a> {
    base_url_path: &'a str,
    matches: Vec<MatchTmplData>,
}

#[get("/matches")]
pub async fn get_matches(req: HttpRequest, info: web::Query<FilterInfo>) -> HttpResult {
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
