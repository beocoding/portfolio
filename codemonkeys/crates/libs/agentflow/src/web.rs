// src/web.rs
pub mod error;
pub mod mw_auth;
pub mod routes_login;
pub mod routes_tickets;
pub mod routes_static;

pub use mw_auth::*;

pub const AUTH_TOKEN: &str = "auth-token";
