// src/log.rs
pub mod error;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::{Method, Uri};
use serde::Serialize;
use serde_json::{json, Value};
use serde_with::{skip_serializing_none};
use uuid::Uuid;
use crate::ctx::Ctx;
use crate::{Error,Result};

use crate::web::error::ClientError;



fn format_iso8601(time: SystemTime) -> String {
    let dur = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let total_secs = dur.as_secs();
    let millis = dur.subsec_millis();

    // Days since Jan 1, 1970
    let days = (total_secs / 86400) as i64;
    let day_secs = total_secs % 86400;

    let hour = day_secs / 3600;
    let minute = (day_secs % 3600) / 60;
    let second = day_secs % 60;

    // Civil day algorithm (Howard Hinnant formula)
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u64; // Day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // Year of era [0, 399]
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // Day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // Month index [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // Day of month [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // Month [1, 12]
    let year = if m <= 2 { y + 1 } else { y };

    return format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

#[skip_serializing_none]
#[derive(Serialize)]
pub struct RequestLogLine {
    uuid: String,
    timestamp: String,
    user_id: Option<u64>,
    req_path: String,
    req_method: String,
    client_error_type: Option<String>,
    error_type: Option<String>,
    error_data: Option<Value>,
}

pub async fn log_request(
    uuid: Uuid,
    req_method: Method,
    uri: Uri,
    ctx: Option<Ctx>,
    service_error: Option<&Error>,
    client_error: Option<ClientError>,
) -> Result<()> {
    let timestamp = format_iso8601(SystemTime::now());
    
    let error_type = service_error.map(|se| se.as_ref().to_string());
    let error_data = serde_json::to_value(service_error)
        .ok()
        .and_then(|mut v| v.get_mut("data").map(|v| v.take()));

    let log_line = RequestLogLine {
        req_path: uri.to_string(),
        req_method: req_method.to_string(),
        uuid: uuid.to_string(),
        timestamp,
        user_id: ctx.map(|c| c.user_id()),
        client_error_type: client_error.map(|e| e.as_ref().to_string()),
        error_type,
        error_data,
    };

    println!("  ->> log_request: \n{}", json!(log_line));

    Ok(())
}