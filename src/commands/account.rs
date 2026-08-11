//! `cpmfl summary` / `balance` / `account` / `properties` — who you are to the
//! association and what you owe it.
//!
//! `summary` and `balance` both emit `utility-summary/v1` (SPEC §1.8): one DTO,
//! two entry points, so a driver like `utiman` needs no per-provider config.

use pk_cli_core::{output, CliError};
use pk_cli_utility::UtilitySummary;
use serde_json::{json, Value};

use super::{emit, emit_list, money, table_view, Ctx};
use crate::config::Service;
use crate::dates::iso_date;

/// `/account` returns one record per ledger the login can see. Nearly every
/// homeowner has exactly one; the rest of this module reads the first unless
/// told otherwise.
pub fn accounts(ctx: &Ctx) -> Result<Vec<Value>, CliError> {
    let client = ctx.client()?;
    let body = client.get(Service::Pay, "/account", &[])?;
    Ok(body.as_array().cloned().unwrap_or_default())
}

/// The account card: balance, next charge, autopay state.
pub fn summary(ctx: &Ctx) -> Result<(), CliError> {
    let list = accounts(ctx)?;
    let first = list
        .first()
        .ok_or_else(|| CliError::NotFound("this login has no association account".into()))?;

    let mut dto = UtilitySummary::new(money(first.get("balance")));
    dto.due_date = first
        .get("nextChargeDate")
        .and_then(Value::as_str)
        .and_then(iso_date);
    dto.account = first
        .get("accountNumber")
        .and_then(Value::as_str)
        .map(str::to_string);
    dto.autopay = first.get("autoDraftEnrolled").and_then(Value::as_bool);

    pk_cli_utility::emit(&dto, ctx.common.json);
    Ok(())
}

/// Everything the portal knows about the account, including the association
/// and property address.
pub fn detail(ctx: &Ctx) -> Result<(), CliError> {
    let list = accounts(ctx)?;
    let first = list
        .first()
        .ok_or_else(|| CliError::NotFound("this login has no association account".into()))?;

    let address = first.get("propertyAddress");
    let payload = json!({
        "account": first.get("accountNumber"),
        "balance": money(first.get("balance")),
        "next_charge_date": first
            .get("nextChargeDate")
            .and_then(Value::as_str)
            .and_then(iso_date),
        "autopay": first.get("autoDraftEnrolled"),
        "association": {
            "id": first.get("associationId"),
            "code": first.get("associationCode"),
            "name": first.get("associationName"),
        },
        "management_company": first.get("managementCompany"),
        "owner": {
            "name": first.get("ownerName"),
            "email": first.get("eMail"),
            "phone": first.get("phoneNumber"),
            "primary": first.get("isPrimaryOwner"),
        },
        "property": {
            "address1": address.and_then(|a| a.get("address1")),
            "address2": address.and_then(|a| a.get("address2")),
            "city": address.and_then(|a| a.get("city")),
            "state": address.and_then(|a| a.get("stateProvince")),
            "postal_code": address.and_then(|a| a.get("postalCode")),
        },
        // Portal-side blocks. Worth surfacing: they're why a balance or ledger
        // can come back empty for reasons that have nothing to do with the CLI.
        "blocked": {
            "ledger": first.get("blockLedger"),
            "payments": first.get("blockPayments"),
            "message": first.get("blockLedgerMessage"),
        },
    });

    emit(ctx, "hoa-account", payload, |v| output::kv(v, 0));
    Ok(())
}

/// Every property/ledger this login can see.
pub fn properties(ctx: &Ctx) -> Result<(), CliError> {
    let client = ctx.client()?;
    let body = client.get(Service::Pay, "/PropertyOwners", &[])?;
    let (raw, total) = super::members(&body);

    let items: Vec<Value> = raw
        .iter()
        .map(|p| {
            let address = p.get("address");
            let association = p.get("association");
            json!({
                "id": p.get("id"),
                "account": p.get("accountNumber"),
                "association": association.and_then(|a| a.get("name")),
                "association_id": association.and_then(|a| a.get("id")),
                "address": address.and_then(|a| a.get("address1")),
                "city": address.and_then(|a| a.get("city")),
                "state": address.and_then(|a| a.get("stateProvince")),
                "postal_code": address.and_then(|a| a.get("postalCode")),
            })
        })
        .collect();

    if ctx.common.json {
        emit_list(ctx, "property", items, total);
    } else {
        output::table(&table_view(
            &items,
            &["account", "association", "address", "city", "state"],
        ));
    }
    Ok(())
}
