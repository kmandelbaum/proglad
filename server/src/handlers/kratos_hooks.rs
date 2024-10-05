use crate::handlers::prelude::*;
use crate::kratos::{kratos_after_registrtation_hook, AccountInfo};

#[post("/kratos_after_registration_hook")]
pub async fn post_kratos_after_registration_hook(
    req: HttpRequest,
    info: web::Json<AccountInfo>,
) -> HttpResult {
    kratos_after_registrtation_hook(req, info).await
}

#[post("/kratos_after_settings_hook")]
pub async fn post_kratos_after_settings_hook(
    req: HttpRequest,
    info: web::Json<AccountInfo>,
) -> HttpResult {
    // Intentionally the same as after registration.
    kratos_after_registrtation_hook(req, info).await
}
