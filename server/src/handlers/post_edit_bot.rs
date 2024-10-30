use crate::handlers::prelude::*;
use sea_orm::Set;

#[derive(Deserialize)]
struct PostEditBotForm {
    set_active: Option<bool>,
}

#[post("/edit_bot/{bot_id}")]
pub async fn post_edit_bot(
    req: HttpRequest,
    session: Session,
    form: web::Form<PostEditBotForm>,
    path: web::Path<i64>,
) -> impl Responder {
    let requester = requester(&req, &session).await?;
    let state = server_state(&req)?;
    let bot_id = *path;
    acl::check(
        &state.db,
        requester,
        db::acls::AccessType::Write,
        db::common::EntityKind::Bot,
        Some(bot_id),
    )
    .await
    .map_err(acl_check_to_http_error)?;
    let new_owner_set_status = match form.set_active {
        None => return Err(AppHttpError::NoEditBotActionSpecified),
        Some(true) => db::bots::OwnerSetStatus::Active,
        Some(false) => db::bots::OwnerSetStatus::Inactive,
    };
    let update = db::bots::ActiveModel {
        id: Set(bot_id),
        owner_set_status: Set(new_owner_set_status),
        ..Default::default()
    };
    db::bots::Entity::update(update)
        .exec(&state.db)
        .await
        .map_err(|e| {
            log::error!("Failed to update bot: {e:?}");
            AppHttpError::Internal
        })?;
    Ok::<_, AppHttpError>(
        web::Redirect::to(format!("{}/bots", state.config.site_base_url_path))
            .see_other()
            .respond_to(&req),
    )
}
