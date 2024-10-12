use crate::handlers::prelude::*;

#[get("/files/{entity_kind}/{entity_id}")]
pub async fn get_files_nameless(
    req: HttpRequest,
    session: Session,
    path: web::Path<(String, i64)>,
) -> HttpResult {
    let path = path.into_inner();
    get_files_impl(req, session, path.0, path.1, String::new()).await
}

#[get("/files/{entity_kind}/{entity_id}/{name}")]
pub async fn get_files(
    req: HttpRequest,
    session: Session,
    path: web::Path<(String, i64, String)>,
) -> HttpResult {
    let path = path.into_inner();
    get_files_impl(req, session, path.0, path.1, path.2).await
}

async fn get_files_impl(
    req: HttpRequest,
    session: Session,
    entity_kind_str: String,
    entity_id: i64,
    name: String,
) -> HttpResult {
    let requester = requester(&req, &session).await?;
    let state = server_state(&req)?;
    let entity_kind = match entity_kind_str.as_str() {
        "game" => db::common::EntityKind::Game,
        "program" => db::common::EntityKind::Program,
        "bot" => db::common::EntityKind::Bot,
        "match" => db::common::EntityKind::Match,
        "account" => db::common::EntityKind::Account,
        _ => return Err(AppHttpError::InvalidEntityKind(entity_kind_str)),
    };
    let file = state
        .file_store
        .read(&state.db, requester, entity_kind, Some(entity_id), &name)
        .await
        .map_err(|e| match e {
            file_store::Error::PermissionDenied => AppHttpError::Unauthorized,
            file_store::Error::NotFound => AppHttpError::NotFound,
            file_store::Error::FileMissingContent
            | file_store::Error::CompressionError(_)
            | file_store::Error::EncodingError
            | file_store::Error::InvalidArgument(_)
            | file_store::Error::DbErr(_) => {
                log::error!("Failed to read file: {e:?}");
                AppHttpError::Internal
            }
        })?;
    // TODO: the client might not be expecting compressed output.
    // Check the request headers for whether they accept compressed.
    let mime = match file.content_type {
        proglad_db::files::ContentType::None => mime::APPLICATION_OCTET_STREAM,
        proglad_db::files::ContentType::PlainText => mime::TEXT_PLAIN,
        proglad_db::files::ContentType::Html => mime::TEXT_HTML,
        proglad_db::files::ContentType::Png => mime::IMAGE_PNG,
        proglad_db::files::ContentType::Svg => mime::IMAGE_SVG,
    };
    let encoding = match file.compression {
        proglad_db::files::Compression::Uncompressed => {
            actix_web::http::header::ContentEncoding::Identity
        }
        proglad_db::files::Compression::Gzip => actix_web::http::header::ContentEncoding::Gzip,
    };
    Ok(HttpResponse::Ok()
        .append_header(ContentType(mime))
        .append_header(encoding)
        .body(file.content.unwrap_or_default()))
}
