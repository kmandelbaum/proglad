use crate::engine;
use crate::handlers::prelude::*;
use crate::validation::*;
use actix_multipart::form::{tempfile::TempFile, text::Text, MultipartForm};

#[derive(Debug, MultipartForm)]
struct CreateBotForm {
    #[multipart(limit = "64KB")]
    file: TempFile,
    language: Text<String>,
    name: Text<String>,
}

#[post("/create_bot/{game_id}")]
pub async fn post_create_bot(
    MultipartForm(form): MultipartForm<CreateBotForm>,
    req: HttpRequest,
    session: Session,
    path: web::Path<i64>,
) -> impl Responder {
    let game_id = *path;
    let state = server_state(&req)?;
    let language = parse_language(&form.language)?;
    let requester = requester(&req, &session).await?;
    crate::acl::check(
        &state.db,
        requester,
        db::acls::AccessType::CreateBotsInGame,
        db::common::EntityKind::Game,
        Some(game_id),
    )
    .await
    .map_err(acl_check_to_http_error)?;
    let Requester::Account(owner) = requester else {
        return Err(AppHttpError::Unauthenticated);
    };
    if let Err(e) = validate_bot_name(&form.name) {
        return Err(AppHttpError::InvalidBotName(e));
    }
    // TODO: move this thing into engine.
    let txn_result = state
        .db
        .transaction(|txn| {
            let file_store = state.file_store.clone();
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

                engine::create_bot(
                    txn,
                    &file_store,
                    game_id,
                    owner,
                    form.file.file,
                    language,
                    &form.name,
                )
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
