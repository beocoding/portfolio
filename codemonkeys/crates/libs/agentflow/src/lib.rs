// src/lib.rs
pub mod ctx;
pub mod error;
pub mod model;
pub mod web;
pub mod log;
pub use error::Error;
pub type Result<T> = core::result::Result<T, error::Error>;
