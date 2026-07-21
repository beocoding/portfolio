// src/model/error.rs
use derive_more::{Display, From};
use serde::Serialize;
use strum_macros::AsRefStr;

#[derive(Debug, Display, From, Clone, AsRefStr, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum Error {
    TicketDeleteFailedIdNotFound {id: u64}
}

// region:    --- Error Boilerplate

impl std::error::Error for Error {}

// endregion: --- Error Boilerplate
