use crate::handlers::prelude::*;

use crate::engine;

#[get("/source/{id}")]
pub async fn get_source(req: HttpRequest, _session: Session, path: web::Path<i64>) -> HttpResult {
    let state = server_state(&req)?;
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
    let source_code = engine::read_source_code(&state.file_store, &state.db, program.id)
        .await
        .map_err(|e| {
            log::error!("Failed to read program {program_id} file: {e}");
            AppHttpError::Internal
        })?;
    Ok(HttpResponse::Ok()
        .append_header(ContentType(mime::TEXT_PLAIN_UTF_8))
        .body(source_code))
}
