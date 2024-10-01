use actix_web::HttpRequest;
use sea_orm::DatabaseConnection;

use crate::http_types::*;

#[derive(Clone)]
pub struct ServerState<'a> {
    pub config: crate::config::ServerConfig,
    pub tmpl: handlebars::Handlebars<'a>,
    pub db: DatabaseConnection,
    pub file_store: crate::file_store::FileStore,
}

pub fn server_state(req: &HttpRequest) -> Result<&ServerState, AppHttpError> {
    req.app_data::<ServerState>().ok_or_else(move || {
        log::error!("Server state is not there");
        AppHttpError::Internal
    })
}
