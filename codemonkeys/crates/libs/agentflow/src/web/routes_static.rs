use axum::{handler::HandlerWithoutStateExt, http::StatusCode, routing::{MethodRouter, any_service}};
use tower_http::services::ServeDir;

pub fn serve_dir(dir: &'static String) -> MethodRouter {
    async fn handle_404() -> (StatusCode, &'static str) {
        (StatusCode::NOT_FOUND, "Resource not found.")
    }
    any_service(
        ServeDir::new(dir).not_found_service(handle_404.into_service()),
    )
}