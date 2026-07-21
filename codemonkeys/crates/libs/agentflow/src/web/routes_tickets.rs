//src/web/routes_tickets.rs
use std::println;

use axum::Router;
use axum::extract::Path;
use axum::routing::{delete, post};
use axum::{Json, extract::State};
use tracing::debug;
use crate::ctx::Ctx;
use crate::error::Result;

use crate::model::ticket::{ModelController, Ticket, TicketForCreate};



pub fn routes(mc: ModelController) -> Router {
    Router::new()
        .route("/tickets", post(create_ticket).get(list_tickets))
        .route("/tickets/{id}", delete(delete_ticket))
        .with_state(mc)
}

// region: REST Handler
async fn create_ticket(
    State(mc): State<ModelController>,
    ctx: Ctx,
    Json(ticket_fc): Json<TicketForCreate>,
) -> Result<Json<Ticket>> {

    debug!(" {:<12} - create_ticket", "HANDLER");

    let ticket = mc.create_ticket(ctx, ticket_fc).await?;

    Ok(Json(ticket))
}

async fn list_tickets(
    State(mc): State<ModelController>,
    ctx: Ctx,
) -> Result<Json<Vec<Ticket>>> {
    debug!(" {:<12} - list_tickets", "HANDLER");
    let tickets = mc.list_tickets(ctx).await?;

    Ok(Json(tickets))
}

async fn delete_ticket(
    State(mc): State<ModelController>,
    ctx: Ctx,
    Path(id): Path<u64>,
) -> Result<Json<Ticket>> {
    println!(">>> {:<15} - delete_ticket", "HANDLER");
    let ticket = mc.delete_ticket(ctx,id).await?;

    Ok(Json(ticket))
}
// endregion: REST Handler