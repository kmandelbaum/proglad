use crate::handlers::prelude::*;

#[get("/replay/{match_id}")]
async fn get_replay(req: HttpRequest, path: web::Path<i64>) -> HttpResult {
    let state = server_state(&req)?;
    // TODO: populate the requester correctly.
    let requester = file_store::Requester::Unauthenticated;
    let file = state
        .file_store
        .read(
            &state.db,
            requester,
            db::files::OwningEntity::Match,
            Some(*path),
            "",
        )
        .await
        .map_err(|e| match e {
            file_store::Error::NotFound => AppHttpError::NotFound,
            file_store::Error::PermissionDenied => AppHttpError::Unauthorized,
            e => {
                log::error!("Failed to read replay file: {e:?}");
                AppHttpError::Internal
            }
        })?;
    let encoding = match file.compression {
        db::files::Compression::Uncompressed => actix_web::http::header::ContentEncoding::Identity,
        db::files::Compression::Gzip => actix_web::http::header::ContentEncoding::Gzip,
    };
    Ok(HttpResponse::Ok()
        .append_header(encoding)
        .body(file.content.unwrap_or_default()))
}
