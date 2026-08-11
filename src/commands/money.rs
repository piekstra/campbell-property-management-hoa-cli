//! `cpmfl transactions` / `payments` / `charges` / `autopay` / `scheduled` /
//! `payment-methods` — the ledger side of the account.
//!
//! Every list here is fetched whole and filtered locally: the portal accepts
//! `page`/`pageSize` but **silently ignores** any date parameter, so a
//! server-side `--since` would return everything while looking filtered. See
//! `docs/api.md`.

use pk_cli_core::{output, CliError};
use pk_cli_utility::RangeArgs;
use serde_json::{json, Value};

use super::{
    emit, emit_list, fetch_all, limited, money, money_cents, money_opt_cents, table_view, Ctx,
};
use crate::config::Service;
use crate::dates::{in_range, iso_date, validate_range};

/// The full ledger: charges, payments, credits, with a running balance.
pub fn transactions(ctx: &Ctx, range: &RangeArgs) -> Result<(), CliError> {
    validate_range(range.since.as_deref(), range.until.as_deref())?;
    let (raw, _) = fetch_all(ctx, Service::Pay, "/Ledger")?;

    let items: Vec<Value> = raw
        .iter()
        .map(|t| {
            let date = t
                .get("ledgerDate")
                .and_then(Value::as_str)
                .and_then(iso_date);
            // Ledger amounts are cents here, unlike every other money field
            // in the API. See `money_cents`.
            json!({
                "date": date,
                "amount": money_cents(t.get("amount")),
                "description": t.get("description"),
                "kind": t.get("transactionType"),
                "running_balance": money_cents(t.get("runningBalance")),
                "voided": t.get("isVoided"),
            })
        })
        .filter(|t| {
            in_range(
                t.get("date").and_then(Value::as_str),
                range.since.as_deref(),
                range.until.as_deref(),
            )
        })
        .collect();

    let total = items.len() as u64;
    let items = limited(items, range.limit);

    if ctx.common.json {
        emit_list(ctx, "transaction", items, Some(total));
    } else {
        output::table(&table_view(
            &items,
            &["date", "kind", "description", "amount", "running_balance"],
        ));
    }
    Ok(())
}

/// Posted and pending payments.
pub fn payments(ctx: &Ctx, range: &RangeArgs) -> Result<(), CliError> {
    validate_range(range.since.as_deref(), range.until.as_deref())?;
    let (raw, _) = fetch_all(ctx, Service::Pay, "/Payment")?;

    let items: Vec<Value> = raw
        .iter()
        .map(|p| {
            json!({
                "id": p.get("id"),
                "date": p.get("date").and_then(Value::as_str).and_then(iso_date),
                "amount": money(p.get("amount")),
                "description": p.get("description"),
                "method": p.get("paymentType"),
                "pending": p.get("pending"),
                "association": p.get("associationName"),
            })
        })
        .filter(|p| {
            in_range(
                p.get("date").and_then(Value::as_str),
                range.since.as_deref(),
                range.until.as_deref(),
            )
        })
        .collect();

    let total = items.len() as u64;
    let items = limited(items, range.limit);

    if ctx.common.json {
        emit_list(ctx, "payment", items, Some(total));
    } else {
        output::table(&table_view(
            &items,
            &["date", "description", "method", "amount", "pending"],
        ));
    }
    Ok(())
}

/// Charges the association has assessed but not yet collected.
pub fn charges(ctx: &Ctx, range: &RangeArgs) -> Result<(), CliError> {
    validate_range(range.since.as_deref(), range.until.as_deref())?;
    let client = ctx.client()?;
    let body = client.get(Service::Pay, "/Charge", &[])?;
    let raw = body.as_array().cloned().unwrap_or_default();

    let items: Vec<Value> = raw
        .iter()
        .map(|c| {
            json!({
                "id": c.get("id").map(|v| v.to_string()),
                "due_date": c.get("dueDate").and_then(Value::as_str).and_then(iso_date),
                "amount": money(c.get("amount")),
                "description": c.get("description"),
            })
        })
        .filter(|c| {
            in_range(
                c.get("due_date").and_then(Value::as_str),
                range.since.as_deref(),
                range.until.as_deref(),
            )
        })
        .collect();

    let total = items.len() as u64;
    let items = limited(items, range.limit);

    if ctx.common.json {
        emit_list(ctx, "statement", items, Some(total));
    } else {
        output::table(&table_view(&items, &["due_date", "description", "amount"]));
    }
    Ok(())
}

/// Autopay (Vantaca calls it AutoDraft) enrollment and schedule — read-only.
pub fn autopay(ctx: &Ctx) -> Result<(), CliError> {
    let client = ctx.client()?;
    let body = client.get(Service::Pay, "/AutoDraft", &[])?;
    let list = body.as_array().cloned().unwrap_or_default();
    let first = list
        .first()
        .ok_or_else(|| CliError::NotFound("no autopay record for this account".into()))?;

    let settings = first.get("autoDraftSettings");
    let payload = json!({
        "enrolled": first.get("enrolled"),
        "account": first.get("accountNumber"),
        "association": first.get("associationName"),
        "start_date": first.get("startDate").and_then(Value::as_str).and_then(iso_date),
        "next_draft_date": first
            .get("nextPullDate")
            .and_then(Value::as_str)
            .and_then(iso_date),
        "bank_account_last4": first.get("bankAccountLast4"),
        "source": first.get("source"),
        "balance": money(first.get("balance")),
        "settings": {
            "draft_day": settings.and_then(|s| s.get("autoDraftDay")),
            "days_in_advance": settings.and_then(|s| s.get("generateDaysInAdvanced")),
            "amount_rule": settings.and_then(|s| s.get("autoDraftAmount")),
            "includes_charges": settings.and_then(|s| s.get("autoDraftIncludeCharges")),
            // Cents, like the ledger — the portal's own UI divides this by
            // 100 before display. Rendering it raw reads as a $299 fee.
            "application_fee": money_cents(settings.and_then(|s| s.get("applicationFee"))),
        },
    });

    emit(ctx, "autopay", payload, |v| output::kv(v, 0));
    Ok(())
}

/// Recurring and one-off future payments already set up — read-only.
///
/// Both records report **cents**, and both express "pay whatever is owed" as
/// `payFullBalance` with a null amount — or a non-null amount that is a *cap*,
/// not the sum to be drafted. Reporting that cap as the amount would state a
/// figure the portal never intends to charge, so the two are separate fields.
pub fn scheduled(ctx: &Ctx) -> Result<(), CliError> {
    let client = ctx.client()?;
    let recurring = client.get(Service::Pay, "/RecurringPayment", &[])?;
    let future = client.get(Service::Pay, "/FuturePayment", &[])?;

    /// Split a raw amount into (amount, max_amount) according to the rule.
    fn amounts(raw: Option<&Value>, full_balance: bool) -> (Value, Value) {
        let money = money_opt_cents(raw);
        if full_balance {
            (Value::Null, money)
        } else {
            (money, Value::Null)
        }
    }

    let mut items: Vec<Value> = recurring
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|r| {
            let bank = r.get("paymentMethod").and_then(|m| m.get("bankAccount"));
            let full_balance = r.get("payFullBalance").and_then(Value::as_bool) == Some(true);
            let (amount, max_amount) = amounts(r.get("amount"), full_balance);
            json!({
                "id": r.get("id"),
                "kind": "recurring",
                "type": r.get("paymentOptionType").or_else(|| r.get("type")),
                "amount": amount,
                "full_balance": full_balance,
                "max_amount": max_amount,
                "interval": r.get("interval"),
                // `nextPaymentDate` is the next draft; `anchorDate` is when the
                // schedule started and is often in the past.
                "next_date": r
                    .get("nextPaymentDate")
                    .and_then(Value::as_str)
                    .and_then(iso_date),
                "started": r.get("anchorDate").and_then(Value::as_str).and_then(iso_date),
                "last_processed": r
                    .get("lastPaymentProcessedDate")
                    .and_then(Value::as_str)
                    .and_then(iso_date),
                "bank_account_last4": bank.and_then(|b| b.get("last4")),
                "cancelled": r.get("cancelled"),
                "notes": r.get("notes"),
            })
        })
        .collect();

    items.extend(
        future
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|f| {
                let bank = f.get("paymentMethod").and_then(|m| m.get("bankAccount"));
                let full_balance = f.get("payFullBalance").and_then(Value::as_bool) == Some(true);
                let (amount, max_amount) = amounts(f.get("amount"), full_balance);
                json!({
                    "id": f.get("id"),
                    "kind": "one-time",
                    "type": f.get("futurePaymentType"),
                    "amount": amount,
                    "full_balance": full_balance,
                    "max_amount": max_amount,
                    // One-off payments date from `scheduledDate`.
                    "next_date": f
                        .get("scheduledDate")
                        .and_then(Value::as_str)
                        .and_then(iso_date),
                    "bank_account_last4": bank.and_then(|b| b.get("last4")),
                    "cancelled": f.get("cancelled"),
                    "notes": f.get("notes"),
                })
            }),
    );

    if ctx.common.json {
        let total = items.len() as u64;
        emit_list(ctx, "scheduled-payment", items, Some(total));
    } else {
        output::table(&table_view(
            &items,
            &[
                "kind",
                "next_date",
                "amount",
                "full_balance",
                "interval",
                "bank_account_last4",
                "cancelled",
            ],
        ));
    }
    Ok(())
}

/// Saved payment methods — last four digits only; the portal never returns more.
pub fn payment_methods(ctx: &Ctx) -> Result<(), CliError> {
    let client = ctx.client()?;
    let body = client.get(Service::Pay, "/PaymentMethod", &[])?;
    let raw = body
        .get("paymentMethods")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let items: Vec<Value> = raw
        .iter()
        .map(|m| {
            let bank = m.get("bankAccount");
            let card = m.get("card");
            json!({
                "id": m.get("id"),
                "kind": if bank.is_some_and(|b| !b.is_null()) { "bank" } else { "card" },
                "last4": bank
                    .and_then(|b| b.get("last4"))
                    .or_else(|| card.and_then(|c| c.get("last4"))),
                "bank_name": bank.and_then(|b| b.get("bankName")),
                "default": m.get("isDefaultPaymentMethod"),
            })
        })
        .collect();

    if ctx.common.json {
        let total = items.len() as u64;
        emit_list(ctx, "payment-method", items, Some(total));
    } else {
        output::table(&table_view(
            &items,
            &["kind", "last4", "bank_name", "default"],
        ));
    }
    Ok(())
}
