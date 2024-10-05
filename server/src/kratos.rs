use actix_session::Session;
use actix_web::web::{self, Redirect};
use actix_web::{HttpRequest, HttpResponse, Responder};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};

use crate::http_types::*;
use crate::server_state::*;
use proglad_db as db;

#[derive(Deserialize, Debug)]
pub struct AccountInfo {
    email: String,
    username: String,
}

pub async fn kratos_after_registrtation_hook(
    req: HttpRequest,
    info: web::Json<AccountInfo>,
) -> HttpResult {
    match crate::validation::validate_account_name(&info.username) {
        Ok(()) => {}
        Err(e) => {
            return Ok(HttpResponse::BadRequest()
                .json(HookResponse::from(e))
                .respond_to(&req))
        }
    }

    let account = db::accounts::ActiveModel {
        email: Set(Some(info.email.to_lowercase())),
        name: Set(info.username.clone()),
        ..Default::default()
    };
    let state = server_state(&req)?;
    let update_result = db::accounts::Entity::insert(account)
        .on_conflict(
            sea_query::OnConflict::column(db::accounts::Column::Email)
                .update_column(db::accounts::Column::Name)
                .to_owned(),
        )
        .exec(&state.db)
        .await;
    match update_result {
        Ok(_) => Ok(HttpResponse::Ok()
            .json(HookResponse::default())
            .respond_to(&req)),
        Err(e) => Ok(HttpResponse::BadRequest()
            .json(HookResponse::from(format!("{e:?}")))
            .respond_to(&req)),
    }
}

pub async fn kratos_authenticate(
    req: &HttpRequest,
    session: &Session,
) -> Result<Option<i64>, AppHttpError> {
    if let Some(account_id) = session.get::<i64>("account_id").map_err(|e| {
        log::error!("Failed to get session: {e:?}");
        AppHttpError::Internal
    })? {
        return Ok(Some(account_id));
    }
    let Some(kratos_session) = kratos_session(req).await? else {
        log::trace!("No kratos session found");
        return Ok(None);
    };
    let Some(kratos_identity) = kratos_session.identity else {
        log::trace!("No identity in kratos kratos session");
        return Ok(None);
    };
    let Some(addresses) = kratos_identity.verifiable_addresses else {
        log::trace!("No verifiable addresses in kratos session");
        return Ok(None);
    };
    if addresses.len() != 1 {
        log::trace!(
            "Unexpected number of verifiable addresses: {}, want 1",
            addresses.len()
        );
        return Ok(None);
    }
    let address = &addresses[0];
    if address.via != ory_kratos_client::models::verifiable_identity_address::ViaEnum::Email {
        log::trace!("Verifiable address isn't an e-mail: {:?}", address.via);
        return Ok(None);
    }
    let email = address.value.to_lowercase();
    let state = server_state(req)?;
    let account = db::accounts::Entity::find()
        .filter(sea_orm::Condition::all().add(db::accounts::Column::Email.eq(&email)))
        .one(&state.db)
        .await
        .map_err(|e| {
            log::error!("Failed to get account for e-mail {email}: {e:?}");
            AppHttpError::Internal
        })?;
    let account = account.ok_or_else(|| {
        let msg = format!("Email {email} not found in the database.");
        log::error!("{msg}");
        AppHttpError::DetailedInternal {
            s: StringError(msg),
        }
    })?;
    let account_id = account.id;
    session.insert("account_id", account_id).map_err(|e| {
        log::error!("Failed to insert account id {account_id} into session: {e:?}");
        AppHttpError::Internal
    })?;
    Ok(Some(account_id))
}

pub async fn kratos_logout<'a>(req: &HttpRequest) -> Result<HttpResponse<()>, AppHttpError> {
    let state = server_state(req)?;
    let flow = ory_kratos_client::apis::frontend_api::create_browser_logout_flow(
        &kratos_config(req)?,
        cookie_str(req),
        None,
    )
    .await;
    match flow {
        Err(e) => {
            log::error!("Failed to create kratos logout flow: {e:?}");
            Ok::<_, AppHttpError>(
                Redirect::to(format!("{}/", state.config.site_base_url_path))
                    .see_other()
                    .respond_to(req),
            )
        }
        Ok(flow) => {
            Ok::<_, AppHttpError>(Redirect::to(flow.logout_url).see_other().respond_to(req))
        }
    }
}

async fn kratos_session(
    req: &HttpRequest,
) -> Result<Option<ory_kratos_client::models::Session>, AppHttpError> {
    // TODO: cache kratos config.
    let kratos_config = kratos_config(req)?;
    let cookies = cookie_str(req);
    match ory_kratos_client::apis::frontend_api::to_session(&kratos_config, None, cookies, None)
        .await
    {
        Ok(kratos_session) => Ok(Some(kratos_session)),
        Err(e) => {
            log::error!("Failed to get kratos session: {e:?}");
            Ok(None)
        }
    }
}

#[derive(Serialize, Default)]
struct HookResponse {
    messages: Vec<HookResponseMessage>,
}

impl From<String> for HookResponse {
    fn from(value: String) -> Self {
        Self {
            messages: vec![HookResponseMessage {
                instance_ptr: "traits.username".to_owned(),
                messages: vec![HookResponseDetailedMessage {
                    id: 1,
                    text: value,
                    r#type: "error".to_owned(),
                }],
            }],
        }
    }
}

#[derive(Serialize)]
struct HookResponseMessage {
    instance_ptr: String,
    messages: Vec<HookResponseDetailedMessage>,
}

#[derive(Serialize)]
struct HookResponseDetailedMessage {
    id: i64,
    text: String,
    r#type: String,
}

fn kratos_config(
    req: &HttpRequest,
) -> Result<ory_kratos_client::apis::configuration::Configuration, AppHttpError> {
    let mut kratos_config = ory_kratos_client::apis::configuration::Configuration::new();
    kratos_config
        .base_path
        .clone_from(&server_state(req)?.config.kratos_api_url);
    let mut headers = reqwest::header::HeaderMap::default();
    headers.insert(
        actix_web::http::header::ACCEPT,
        actix_web::http::header::HeaderValue::from_static("application/json"),
    );
    kratos_config.client = reqwest::ClientBuilder::new()
        .default_headers(headers)
        .build()
        .map_err(|e| {
            log::error!("kratos_authenticate: Failed to build reqwest client: {e}");
            AppHttpError::Internal
        })?;
    Ok(kratos_config)
}

fn cookie_str(req: &HttpRequest) -> Option<&str> {
    req.headers()
        .get(actix_web::http::header::COOKIE)?
        .to_str()
        .inspect_err(|e| {
            log::error!("kratos_authenticate: Failed to convert cookie to str: {e}");
        })
        .ok()
}
