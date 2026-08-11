//! `cpmfl directory` / `calendar` / `amenities` / `association` — the
//! community-facing reads that aren't about money.

use pk_cli_core::{output, CliError};
use serde_json::{json, Value};

use super::{emit, emit_list, fetch_all, limited, table_view, Ctx};
use crate::config::Service;
use crate::dates::{in_range, iso_date, validate_range};

/// Board members and management contacts.
pub fn directory(ctx: &Ctx, limit: Option<u32>) -> Result<(), CliError> {
    let client = ctx.client()?;
    let association = ctx.association_id(&client)?;
    // Paged like everything else: without explicit paging the portal caps at
    // 10, quietly hiding the rest of a larger board.
    let (raw, total) = fetch_all(
        ctx,
        Service::Associations,
        &format!("/associations/{association}/directory"),
    )?;

    let items: Vec<Value> = raw
        .iter()
        .map(|d| {
            json!({
                "id": d.get("directoryId"),
                "name": d.get("displayName"),
                "role": d.get("roleName"),
                "description": d.get("boardDescription"),
                "email": d.get("email").filter(|v| v.as_str() != Some("")),
                "phone": d.get("phone").filter(|v| v.as_str() != Some("")),
            })
        })
        .collect();
    let items = limited(items, limit);

    if ctx.common.json {
        emit_list(ctx, "directory-entry", items, total);
    } else {
        output::table(&table_view(&items, &["name", "role", "email", "phone"]));
    }
    Ok(())
}

/// Community calendar events.
pub fn calendar(
    ctx: &Ctx,
    since: Option<&str>,
    until: Option<&str>,
    limit: Option<u32>,
) -> Result<(), CliError> {
    validate_range(since, until)?;
    let client = ctx.client()?;
    let association = ctx.association_id(&client)?;
    let (raw, _) = fetch_all(
        ctx,
        Service::Associations,
        &format!("/associations/{association}/community-calendar"),
    )?;

    let items: Vec<Value> = raw
        .iter()
        .map(|e| {
            json!({
                "id": e.get("id"),
                "title": e.get("title").or_else(|| e.get("name")),
                "start": e.get("startDate").and_then(Value::as_str).and_then(iso_date),
                "end": e.get("endDate").and_then(Value::as_str).and_then(iso_date),
                "category": e.get("categoryDescription").or_else(|| e.get("category")),
                "location": e.get("location"),
            })
        })
        .filter(|e| in_range(e.get("start").and_then(Value::as_str), since, until))
        .collect();

    let total = items.len() as u64;
    let items = limited(items, limit);

    if ctx.common.json {
        emit_list(ctx, "event", items, Some(total));
    } else {
        output::table(&table_view(
            &items,
            &["start", "title", "category", "location"],
        ));
    }
    Ok(())
}

/// Amenity reservations you hold.
///
/// Associations that haven't turned reservations on answer 403 with an
/// explanatory message; the client maps that to exit 4 (not found) so it reads
/// as "not offered here" rather than "log in again".
pub fn reservations(ctx: &Ctx) -> Result<(), CliError> {
    let client = ctx.client()?;
    let body = client.get(Service::ActionItems, "/Amenities/MyReservations", &[])?;
    let raw = body
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let items: Vec<Value> = raw
        .iter()
        .map(|r| {
            json!({
                "id": r.get("id"),
                "amenity": r.get("amenityName").or_else(|| r.get("name")),
                "date": r.get("startDate").and_then(Value::as_str).and_then(iso_date),
                "status": r.get("status"),
            })
        })
        .collect();

    if ctx.common.json {
        let total = items.len() as u64;
        emit_list(ctx, "reservation", items, Some(total));
    } else {
        output::table(&table_view(&items, &["date", "amenity", "status"]));
    }
    Ok(())
}

/// What the association has switched on, and the portal features available.
pub fn association(ctx: &Ctx) -> Result<(), CliError> {
    let client = ctx.client()?;
    let id = ctx.association_id(&client)?;
    let features = client.get(
        Service::Associations,
        &format!("/associations/{id}/features"),
        &[],
    )?;
    let pay_features = client.get(Service::Pay, "/Features", &[])?;
    let feed = client
        .get(
            Service::Associations,
            &format!("/associations/{id}/community-feed/config"),
            &[],
        )
        .unwrap_or(Value::Null);

    let payload = json!({
        "association_id": id,
        "timezone_offset": features.get("timezoneOffset"),
        "features": {
            "vantaca_pay": pay_features.get("vantacaPay"),
            "express_pay": pay_features.get("expressPay"),
            "guest_pay": pay_features.get("guestPay"),
            "sms": pay_features.get("smsIntegration"),
            "community_feed": feed.get("enableCommunityFeed"),
            "board_posting": feed.get("enableBoardPosting"),
        },
        "domain": pay_features.get("domain"),
    });

    emit(ctx, "association", payload, |v| output::kv(v, 0));
    Ok(())
}
