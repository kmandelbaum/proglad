use crate::handlers::prelude::*;

#[derive(Serialize)]
struct MainPageTmplData<'a> {
    base_url_path: &'a str,
    auth_url: &'a str,
    authenticated: bool,
    account_id: i64,
}

#[get("/")]
pub async fn get_index(req: HttpRequest, session: Session) -> HttpResult {
    let state = server_state(&req)?;
    let config = &state.config;
    let account_id = kratos_authenticate(&req, &session).await?;
    let html = state
        .tmpl
        .render(
            "main",
            &MainPageTmplData {
                base_url_path: &config.site_base_url_path,
                auth_url: &config.auth_base_url,
                authenticated: account_id.is_some(),
                account_id: account_id.unwrap_or_default(),
            },
        )
        .map_err(|e| {
            log::error!("Failed to render main page template: {e}");
            AppHttpError::Internal
        })?;
    Ok(HttpResponse::Ok()
        .append_header(ContentType(mime::TEXT_HTML))
        .body(html))
}
