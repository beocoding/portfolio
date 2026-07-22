// src/main.rs
use agent_flow::{config, ctx::Ctx, log::log_request, model::ticket::ModelController, web::{self, routes_login, routes_static::{self, serve_dir}}};
use axum::{
    Json, Router, http::{Method, Uri}, middleware, response::{IntoResponse, Response},
};
use serde_json::json;
use tower_cookies::CookieManagerLayer;
use tracing::debug;
use uuid::Uuid;
use std::{net::SocketAddr, println};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;
use tracing_subscriber::EnvFilter;
use agent_flow::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .without_time()
        .with_target(false)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let mc = ModelController::new().await?;

    let api_routes = web::routes_tickets::routes(mc.clone())
        .route_layer(middleware::from_fn(web::mw_auth::mw_require_auth));

    let all_routes = Router::new()
        .merge(routes_login::routes())
        .nest("/api", api_routes)
        .layer(middleware::map_response(main_response_mapper))
        .layer(middleware::from_fn_with_state(
            mc.clone(), 
            web::mw_auth::mw_ctx_resolver)
        )
        .layer(CookieManagerLayer::new())
        .fallback_service(serve_dir(&config().WEB_FOLDER));

    // region: Start Server
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    debug!(" {:<12} - {addr}\n", "LISTENING");
    let listener = TcpListener::bind(&addr).await?;

    axum::serve(listener, all_routes).await?;
    // endregion: Start Server

    Ok(())
}

pub fn routes_static() -> Router {
    Router::new().fallback_service(ServeDir::new("./"))
}


async fn main_response_mapper(
    ctx: Option<Ctx>,
    uri: Uri,
    req_method: Method,
    res: Response
) -> Response {
    debug!(" {:<12} - main_response_mapper", "RES_MAPPER");
    let uuid = Uuid::new_v4();

    // -- Get the eventual response error
    let service_error = res.extensions().get::<Error>();
    let client_status_error = service_error.map(|se| se.client_status_and_error());

    // --If client error, build the new response
    let error_response = 
        client_status_error
            .as_ref()
            .map(|(status_code, client_error)| {
                let client_error_body = json!({
                    "error": {
                        "type": client_error.as_ref(),
                        "req_uuid": uuid.to_string(),
                }
            });
            println!("  ->> client_error_body: {client_error_body}");

            // Build new response
            (*status_code, Json(client_error_body)).into_response()
            
        });

    
    // Build and log the server log line
    let client_error = client_status_error.unzip().1;
    _ = log_request(uuid, req_method, uri, ctx, service_error, client_error).await;
    error_response.unwrap_or(res)
}