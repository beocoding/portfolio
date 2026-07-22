// src/error.rs
use axum::{http::StatusCode, response::IntoResponse};
use derive_more::{Display, From};
use serde::Serialize;
use strum_macros::AsRefStr;
use tracing::debug;

use crate::web::error::{Error as WebError, ClientError};
use crate::model::error::{Error as ModelError};


pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Display, From, Clone, AsRefStr, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum Error {
    // Explicitly use the full path so it doesn't clash with the name `Error`
    #[from]
    WebError(WebError),
    #[from]
    ClientError(ClientError),
    
    // Explicitly use the full path here too
    #[from]
    ModelError(ModelError),

    ConfigMissingEnv(&'static str)
}

// region:    --- Custom


// endregion: --- Custom

// region:    --- Error Boilerplate

impl std::error::Error for Error {}

// endregion: --- Error Boilerplate

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        debug!(" {:<12} - {self:?}", "INTO_RESPONSE");
        
        // Create placeholder Axum response
        let mut response = StatusCode::INTERNAL_SERVER_ERROR.into_response();

        response.extensions_mut().insert(self);

        response
    }
}



impl Error {
    pub fn client_status_and_error(&self) -> (StatusCode, ClientError) {
        #[allow(unreachable_patterns)]
        match self {
            // -- Auth
            Self::WebError(WebError::AuthFailNoAuthTokenCookie)|
            Self::WebError(WebError::AuthFailTokenWrongFormat)|
            Self::WebError(WebError::AuthFailCtxNotInRequestExt) => {
                (StatusCode::FORBIDDEN, ClientError::NO_AUTH)
            }
            // -- Model
            Self::ModelError(ModelError::TicketDeleteFailedIdNotFound { .. }) => {
                (StatusCode::BAD_REQUEST, ClientError::INVALID_PARAMS)
            }
            // -- Fallback
            _=> (
                StatusCode::INTERNAL_SERVER_ERROR,
                ClientError::SERVICE_ERROR,
            ),
        }
    }
}