use crate::handlers::prelude::*;

#[derive(Serialize)]
struct VisualizerTmplData<'a> {
    base_url_path: &'a str,
    match_id: i64,
    match_data: MatchTmplData,
}

#[get("/visualizer/{match_id}")]
pub async fn get_visualizer(req: HttpRequest, path: web::Path<i64>) -> HttpResult {
    let state = server_state(&req)?;
    let matches = db::matches::Entity::find_by_id(*path)
        .all(&state.db)
        .await
        .map_err(|e| {
            log::error!("Failed to get match {}: {e:?}", *path);
            AppHttpError::NotFound
        })?;
    let match_data = match_tmpl_data(&state.db, &matches, |_| false).await?;
    if match_data.len() != 1 {
        log::error!(
            "Failed to construct match template data: found {} match tmpl data, expected 1.",
            match_data.len()
        );
        return Err(AppHttpError::Internal);
    }
    let html = state
        .tmpl
        .render(
            "visualizer",
            &VisualizerTmplData {
                base_url_path: &state.config.site_base_url_path,
                match_id: *path,
                match_data: match_data.into_iter().next().unwrap(),
            },
        )
        .map_err(|e| {
            log::error!("Failed to render template: {e}");
            AppHttpError::Internal
        })?;
    Ok(HttpResponse::Ok()
        .append_header(ContentType(mime::TEXT_HTML))
        .body(html))
}
