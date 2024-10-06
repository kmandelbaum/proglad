use actix_web::http::{header::ContentType, StatusCode};
use actix_web::HttpResponse;
use derive_more::Display;

pub type HttpResult = Result<HttpResponse, AppHttpError>;

#[derive(Debug, Display)]
pub enum AppHttpError {
    #[display(fmt = "Internal error.")]
    Internal,

    #[display(fmt = "Internal error: {_0}")]
    DetailedInternal(String),

    #[display(fmt = "Bad request.")]
    BadClientData,

    #[display(fmt = "Not found.")]
    NotFound,

    #[display(fmt = "Unauthenticated.")]
    Unauthenticated,

    #[display(fmt = "Unauthorized.")]
    Unauthorized,

    #[display(fmt = "Bot with the given name already exists. Choose a different name.")]
    BotAlreadyExists,

    #[display(fmt = "Invalid bot name: {_0}")]
    InvalidBotName(String),

    #[display(fmt = "Invalid entity kind: {_0}")]
    InvalidEntityKind(String),
}

impl std::error::Error for AppHttpError{}

impl actix_web::error::ResponseError for AppHttpError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::html())
            .body(self.to_string())
    }

    fn status_code(&self) -> StatusCode {
        match *self {
            AppHttpError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            AppHttpError::DetailedInternal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppHttpError::NotFound => StatusCode::NOT_FOUND,
            AppHttpError::BadClientData => StatusCode::BAD_REQUEST,
            AppHttpError::Unauthenticated => StatusCode::UNAUTHORIZED,
            AppHttpError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppHttpError::BotAlreadyExists => StatusCode::CONFLICT,
            AppHttpError::InvalidBotName(_) => StatusCode::BAD_REQUEST,
            AppHttpError::InvalidEntityKind(_) => StatusCode::BAD_REQUEST,
        }
    }
}
