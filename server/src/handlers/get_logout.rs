use crate::handlers::prelude::*;
use crate::kratos::kratos_logout;

#[derive(Deserialize)]
struct LogoutInfo {
    finished: Option<bool>,
}

#[get("/logout")]
pub async fn get_logout(
    req: HttpRequest,
    session: Session,
    info: web::Query<LogoutInfo>,
) -> impl Responder {
    session.purge();
    if info.finished == Some(true) {
        return Ok::<_, AppHttpError>(
            web::Redirect::to(format!(
                "{}/",
                server_state(&req)?.config.site_base_url_path
            ))
            .see_other()
            .respond_to(&req),
        );
    }
    kratos_logout(&req).await
}
