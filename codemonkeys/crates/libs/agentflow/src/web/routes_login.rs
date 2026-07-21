// src/web/routes_login.rs
use std::println;

use crate::web::error::Error::LoginFail;
use crate::web::AUTH_TOKEN;
use crate::Result;

use axum::{Json, Router, routing::post};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_cookies::{Cookie, Cookies};
use tracing::debug;

#[derive(Debug,Deserialize)]
pub struct LoginPayload {
    username: String,
    pwd: String,
}

pub fn routes() -> Router{
    Router::new()
        .route("/api/login", post(api_login))
}

async fn api_login(
    cookies: Cookies,
    payload: Json<LoginPayload>
) -> Result<Json<Value>> {
    debug!(" {:<12} - api_login", "HANDLER");

    // TODO: Implement real db/auth logic
    if payload.username != "demo1" || payload.pwd != "welcome" {
        return Err(LoginFail.into());
    }


    // FIXME: Implement real auth-token generation/signature
    cookies.add(Cookie::new(AUTH_TOKEN, "user-1.exp.sign"));

    let body = Json(json!({
        "result": {
            "success": true
        }
    }));

    Ok(body)
}
