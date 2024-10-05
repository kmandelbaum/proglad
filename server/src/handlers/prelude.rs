pub use std::collections::{HashMap, HashSet};

pub use actix_session::Session;
pub use actix_web::http::header::ContentType;
pub use actix_web::{get, post, web, HttpRequest, HttpResponse, Responder};
pub use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
    TransactionTrait,
};
pub use sea_query::IntoCondition;
pub use serde::{Deserialize, Serialize};

pub use proglad_db as db;

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
