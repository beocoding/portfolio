#![allow(unused)]
use std::println;

use anyhow::Result;
use axum::response::IntoResponse;
use serde_json::json;

#[tokio::test]
async fn quick_dev() -> Result<()> {
    let hc = httpc_test::new_client("http://localhost:8080")?;
    hc.do_get("/index.html").await?.print().await?;
    let req_login = hc.do_post(
        "/api/login",
        json!({
            "username": "demo1",
            "pwd": "welcome"
        })
    );
    req_login.await?.print().await?;
    Ok(())
}

