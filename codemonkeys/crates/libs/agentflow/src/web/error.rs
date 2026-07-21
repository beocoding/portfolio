// src/web/error.rs
use derive_more::{Display, From};
use serde::Serialize;
use strum_macros::AsRefStr;

#[derive(Debug, Display, From, Clone, AsRefStr, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum Error {
    LoginFail,
    AuthFailNoAuthTokenCookie,
    AuthFailTokenWrongFormat,
    AuthFailCtxNotInRequestExt,
}



#[derive(Debug, Display, From, Clone, AsRefStr, Serialize)]
#[serde(tag = "type", content = "data")]
#[allow(non_camel_case_types)]
pub enum ClientError {
    LOGIN_FAIL,
    NO_AUTH,
    INVALID_PARAMS,
    SERVICE_ERROR,
}

impl std::error::Error for Error {}