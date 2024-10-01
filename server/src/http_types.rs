use actix_web::http::{header::ContentType, StatusCode};
use actix_web::HttpResponse;
use derive_more::{Display, Error};

pub type HttpResult = Result<HttpResponse, AppHttpError>;

#[derive(Debug)]
pub struct StringError(pub String);

impl std::error::Error for StringError {}
impl std::fmt::Display for StringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Display, Error)]
pub enum AppHttpError {
    #[display(fmt = "Internal error.")]
    Internal,

    #[display(fmt = "Internal error: {}", s)]
    DetailedInternal { s: StringError },

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

    #[display(fmt = "Invalid bot name: {}", s)]
    InvalidBotName { s: StringError },
}

impl actix_web::error::ResponseError for AppHttpError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::html())
            .body(self.to_string())
    }

    fn status_code(&self) -> StatusCode {
        match *self {
            AppHttpError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            AppHttpError::DetailedInternal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            AppHttpError::NotFound => StatusCode::NOT_FOUND,
            AppHttpError::BadClientData => StatusCode::BAD_REQUEST,
            AppHttpError::Unauthenticated => StatusCode::UNAUTHORIZED,
            AppHttpError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppHttpError::BotAlreadyExists => StatusCode::CONFLICT,
            AppHttpError::InvalidBotName { .. } => StatusCode::BAD_REQUEST,
        }
    }
}
