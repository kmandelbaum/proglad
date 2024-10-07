use crate::handlers::prelude::*;
use sea_orm::Condition;

#[post("/schedule_match/{game_id}")]
pub async fn post_schedule_match(
    req: HttpRequest,
    session: Session,
    path: web::Path<i64>,
) -> Result<HttpResponse<()>, AppHttpError> {
    let state = server_state(&req)?;
    let game_id = *path;
    let requester = requester(&req, &session).await?;
    // Write acl on the game to check that this requester can start matches for this game.
    acl::check(
        &state.db,
        requester,
        db::acls::AccessType::Write,
        db::common::EntityKind::Game,
        Some(game_id),
    )
    .await
    .map_err(acl_check_to_http_error)?;

    let already_scheduled = db::work_items::Entity::find()
        .filter(
            Condition::all()
                .add(db::work_items::Column::Status.eq(db::work_items::Status::Scheduled))
                .add(db::work_items::Column::WorkType.eq(db::work_items::WorkType::RunMatch))
                .add(db::work_items::Column::GameId.eq(Some(game_id))),
        )
        .order_by_desc(db::work_items::Column::CreationTime)
        .limit(1)
        .all(&state.db)
        .await
        .map_err(|e| {
            log::error!("Failed to check already scheduled matches: {e:?}");
            AppHttpError::Internal
        })?;
    if !already_scheduled.is_empty() {
        return Err(AppHttpError::MatchAlreadyScheduled);
    }
    // TODO - configurable priority.
    crate::engine::schedule_match_for_game(&state.db, game_id, 2000)
        .await
        .map_err(|e| {
            log::error!("Failed to schedule match for game {game_id}: {e:?}");
            AppHttpError::Internal
        })?;
    Ok(web::Redirect::to(format!(
        "{}/edit_game?game_id={game_id}",
        state.config.site_base_url_path
    ))
    .see_other()
    .respond_to(&req))
}
