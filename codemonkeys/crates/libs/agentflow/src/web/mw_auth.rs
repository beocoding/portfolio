//src/web/mw_auth.rs
use std::matches;

use axum::{extract::{FromRequestParts, OptionalFromRequestParts, Request}, http::request::Parts, middleware::Next, response::Response};
use lazy_regex::regex_captures;
use tower_cookies::{Cookie, Cookies};
use tracing::debug;

use crate::{Result, ctx::Ctx, error::Error, web::{AUTH_TOKEN, error::Error::{AuthFailCtxNotInRequestExt, AuthFailNoAuthTokenCookie, AuthFailTokenWrongFormat}}};

// Removed <B> from the function, Request, and Next
pub async fn mw_require_auth(
    ctx: Result<Ctx>,
    req: Request,
    next: Next,
) -> Result<Response> {
    debug!(" {:<12} - mw_require_auth - {ctx:?}", "MIDDLEWARE");

    ctx?;

    Ok(next.run(req).await)    
}
pub async fn mw_ctx_resolver(
    cookies: Cookies,
    mut req: Request,
    next: Next,
) -> Result<Response> {
    debug!(" {:<12} - mw_ctx_resolver ", "MIDDLEWARE");
    let auth_token = cookies.get(AUTH_TOKEN).map(|c| c.value().to_string());

    // Compute Result<Ctx>
    let result_ctx = match auth_token
        .ok_or(Error::from(AuthFailNoAuthTokenCookie))
        .and_then(parse_token)
    {
        Ok((user_id, _exp, _sign)) => {
            // TODO: Token components validations
            Ok(Ctx::new(user_id))
        }
        Err(e) => Err(e),
    };

    // Remove cookie if something went wrong other than NoAuthTokenCookie
    if result_ctx.is_err()
        && !matches!(
            result_ctx,
            Err(crate::error::Error::WebError(AuthFailNoAuthTokenCookie))
        )
    {
        cookies.remove(Cookie::from(AUTH_TOKEN))
    }

    // Store the ctx_result in the request extension
    req.extensions_mut().insert(result_ctx);
    Ok(next.run(req).await)     
}

/// Parse a token with format 'user-[user-id].[expiration].[signature]
/// Returns (user_id, expiration, signature)
fn parse_token(token: String) -> Result<(u64, String, String)> {
    let (_whole, user_id, exp, sign) = regex_captures!(
        r#"^user-(\d+)\.(.+)\.(.+)"#,
        &token
    )
    .ok_or(Error::from(AuthFailTokenWrongFormat))?;

    let user_id: u64 = user_id.parse().map_err(|_| Error::from(AuthFailTokenWrongFormat))?;
    Ok((user_id, exp.to_string(), sign.to_string()))
}

// region: Ctx Extracter
impl<S: Send + Sync> FromRequestParts<S> for Ctx {
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> Result<Self> {
        
        debug!(" {:<12} - Ctx", "EXTRACTOR");

        parts
            .extensions
            .get::<Result<Ctx>>()
            .ok_or(Error::from(AuthFailCtxNotInRequestExt))?
            .clone()

    }
}  

impl<S: Send + Sync> OptionalFromRequestParts<S> for Ctx {
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> Result<Option<Self>> {
        debug!(" {:<12} - Ctx (optional)", "EXTRACTOR");

        Ok(parts
            .extensions
            .get::<Result<Ctx>>()
            .and_then(|res| res.clone().ok()))
    }
}
// endregion: Ctx Extracter
