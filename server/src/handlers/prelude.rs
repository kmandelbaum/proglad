pub use std::collections::{HashMap, HashSet};

pub use actix_session::Session;
pub use actix_web::http::header::ContentType;
pub use actix_web::{get, post, web, HttpRequest, HttpResponse, Responder};
pub use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
    TransactionTrait, ActiveEnum
};
pub use sea_query::IntoCondition;
pub use serde::{Deserialize, Serialize};

pub use proglad_db as db;

pub use crate::acl::{self, Requester};
pub use crate::file_store;
pub use crate::handlers::tmpl_data::*;
pub use crate::http_types::*;
pub use crate::kratos::kratos_authenticate;
pub use crate::server_state::*;

#[derive(Deserialize, Debug)]
pub struct FilterInfo {
    pub account_id: Option<i64>,
    pub game_id: Option<i64>,
}

pub async fn requester(req: &HttpRequest, session: &Session) -> Result<Requester, AppHttpError> {
    let state = server_state(req)?;
    match kratos_authenticate(req, session).await? {
        Some(o) => Ok(Requester::Account(o)),
        None => {
            if let Some(default_account_name) =
                &state.config.access_control.insecure_default_account
            {
                // TODO: cache this in state.
                let Some(account_id) = db::accounts::Entity::find()
                    .filter(db::accounts::Column::Name.eq(default_account_name))
                    .select_only()
                    .column(db::accounts::Column::Id)
                    .into_values::<i64, db::accounts::Column>()
                    .one(&state.db)
                    .await
                    .map_err(|e| {
                        log::error!("Failed to fetch default account: {e}");
                        AppHttpError::Internal
                    })?
                else {
                    log::error!("Account name not found: {default_account_name}");
                    return Err(AppHttpError::Unauthenticated);
                };
                Ok(Requester::Account(account_id))
            } else {
                Ok(Requester::Unauthenticated)
            }
        }
    }
}

pub fn language_choices(selected: Option<db::programs::Language>) -> Vec<LanguageChoice> {
    // TODO: properly support Java.
    use sea_orm::strum::IntoEnumIterator;
    db::programs::Language::iter()
        .filter(|lang| *lang != db::programs::Language::Java)
        .map(|lang| LanguageChoice {
            value: lang.to_value(),
            name: lang.as_str().to_owned(),
            selected: selected == Some(lang),
        })
        .collect()
}

pub fn acl_check_to_http_error(err: acl::Error) -> AppHttpError {
    log::error!("Error while checking ACL: {err:?}");
    match err {
        acl::Error::Denied => AppHttpError::Unauthorized,
        acl::Error::NotFound(_) => AppHttpError::NotFound,
        acl::Error::DbErr(_) => AppHttpError::Internal,
        acl::Error::InvalidArgument(_) => AppHttpError::Internal,
    }
}
