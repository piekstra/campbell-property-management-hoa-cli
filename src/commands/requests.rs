//! `cpmfl requests` — homeowner service requests (work orders, ARC
//! applications, general questions) and the request types the association
//! accepts.
//!
//! Reads only. Submitting a request is a portal capability this CLI
//! deliberately does not expose — see `docs/api.md` for the endpoints it would
//! use.

use pk_cli_core::{output, CliError};
use serde_json::{json, Value};

use super::{emit_list, fetch_all, limited, table_view, Ctx};
use crate::config::Service;
use crate::dates::{in_range, iso_date, validate_range};

#[derive(clap::Subcommand, Debug)]
pub enum Cmd {
    /// Requests you have submitted.
    List {
        /// Only requests submitted on or after this date (ISO `YYYY-MM-DD`).
        #[arg(long, value_name = "YYYY-MM-DD")]
        since: Option<String>,
        /// Only requests submitted on or before this date (ISO `YYYY-MM-DD`).
        #[arg(long, value_name = "YYYY-MM-DD")]
        until: Option<String>,
        /// Include requests the association has closed.
        #[arg(long)]
        all: bool,
        /// Maximum requests to return.
        #[arg(long, value_name = "N")]
        limit: Option<u32>,
    },
    /// Request categories this association accepts.
    Types,
}

pub fn run(ctx: &Ctx, cmd: &Cmd) -> Result<(), CliError> {
    match cmd {
        Cmd::List {
            since,
            until,
            all,
            limit,
        } => list(ctx, since.as_deref(), until.as_deref(), *all, *limit),
        Cmd::Types => types(ctx),
    }
}

fn list(
    ctx: &Ctx,
    since: Option<&str>,
    until: Option<&str>,
    all: bool,
    limit: Option<u32>,
) -> Result<(), CliError> {
    validate_range(since, until)?;
    let (raw, _) = fetch_all(ctx, Service::ActionItems, "/HomeownerRequests")?;

    let items: Vec<Value> = raw
        .iter()
        .map(|r| {
            json!({
                "id": r.get("id"),
                "submitted": r.get("submitted").and_then(Value::as_str).and_then(iso_date),
                "type": r.get("type"),
                "category": r.get("category"),
                "subject": r.get("subject"),
                "status": r.get("status"),
                "closed": r.get("closed"),
            })
        })
        .filter(|r| all || r.get("closed").and_then(Value::as_bool) != Some(true))
        .filter(|r| in_range(r.get("submitted").and_then(Value::as_str), since, until))
        .collect();

    let total = items.len() as u64;
    let items = limited(items, limit);

    if ctx.common.json {
        emit_list(ctx, "request", items, Some(total));
    } else {
        output::table(&table_view(
            &items,
            &["id", "submitted", "type", "subject", "status"],
        ));
    }
    Ok(())
}

fn types(ctx: &Ctx) -> Result<(), CliError> {
    let client = ctx.client()?;
    let body = client.get(
        Service::ActionItems,
        "/Home/GeneralRequests/RequestTypes",
        &[],
    )?;
    let raw = body
        .get("requestTypes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let items: Vec<Value> = raw
        .iter()
        .map(|t| {
            json!({
                "id": t.get("actionTypeId"),
                "description": t.get("description"),
                "category": t.get("categoryName"),
                "requires_association": t.get("requiresAssociation"),
            })
        })
        .collect();

    if ctx.common.json {
        let total = items.len() as u64;
        emit_list(ctx, "request-type", items, Some(total));
    } else {
        output::table(&table_view(&items, &["id", "category", "description"]));
    }
    Ok(())
}
