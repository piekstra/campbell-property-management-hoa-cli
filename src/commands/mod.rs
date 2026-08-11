//! Domain command modules. Each read emits a schema-tagged DTO in `--json`
//! mode and a shaped table/kv view in text mode.

pub mod account;
pub mod api;
pub mod community;
pub mod documents;
pub mod money;
pub mod profile;
pub mod requests;

use pk_cli_core::{output, CliError, CommonArgs, Money};
use pk_cli_secrets::CredentialStore;
use serde_json::Value;

use crate::client::Portal;
use crate::config::{Config, Service};

pub struct Ctx<'a> {
    pub common: &'a CommonArgs,
    pub cfg: &'a Config,
    pub creds: &'a CredentialStore,
}

impl Ctx<'_> {
    /// A portal session replayed from the keychain. An expired or missing
    /// token surfaces as `CliError::Auth` (exit 3), pointing at `auth login`.
    pub fn client(&self) -> Result<Portal, CliError> {
        Portal::from_cached_token(self.cfg, self.creds)
    }

    /// The association to scope association-level reads to: config first, then
    /// whatever the account record reports.
    pub fn association_id(&self, portal: &Portal) -> Result<u64, CliError> {
        if let Some(id) = self.cfg.association_id {
            return Ok(id);
        }
        let accounts = portal.get(crate::config::Service::Pay, "/account", &[])?;
        accounts
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.get("associationId"))
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                CliError::NotFound(
                    "could not determine the association for this login — set one with \
                     `cpmfl config set association_id <id>`"
                        .into(),
                )
            })
    }

    /// The homeowner id this login maps to, read from the account record.
    /// Preferences are keyed by homeowner id, and the portal 404s ("does not
    /// have access") for any other id — including the owner id — so it must be
    /// exactly this one.
    pub fn homeowner_id(&self, portal: &Portal) -> Result<u64, CliError> {
        let accounts = portal.get(crate::config::Service::Pay, "/account", &[])?;
        accounts
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.get("homeownerId"))
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                CliError::NotFound("could not determine the homeowner for this login".into())
            })
    }
}

/// Emit a DTO: tagged payload in JSON mode, rendered view in text mode.
pub fn emit(ctx: &Ctx, schema: &str, payload: Value, text: impl FnOnce(&Value)) {
    if ctx.common.json {
        let mut tagged = serde_json::Map::new();
        tagged.insert("schema".into(), Value::String(format!("{schema}/v1")));
        match payload {
            Value::Object(m) => tagged.extend(m),
            other => {
                tagged.insert("data".into(), other);
            }
        }
        output::json(&Value::Object(tagged));
    } else {
        text(&payload);
    }
}

/// Emit a list DTO under the family's `Paged` envelope (`<record>-list/v1`).
pub fn emit_list(ctx: &Ctx, record: &str, items: Vec<Value>, total: Option<u64>) {
    let mut paged = pk_cli_utility::Paged::new(record, items);
    paged.total = total;
    paged.emit(ctx.common.json);
}

/// Pull selected columns out of an array of objects for table rendering.
pub fn table_view(items: &[Value], columns: &[&str]) -> Vec<Value> {
    items
        .iter()
        .map(|item| {
            let mut row = serde_json::Map::new();
            for col in columns {
                if let Some(v) = item.get(*col) {
                    row.insert((*col).to_string(), v.clone());
                }
            }
            Value::Object(row)
        })
        .collect()
}

/// The portal returns money as a JSON number; the contract wants string-decimal
/// [`Money`]. Absent/unstructured values become `0.00` rather than an error —
/// a missing amount is a reporting gap, not a reason to fail a whole list.
pub fn money(v: Option<&Value>) -> Money {
    let amount = v.and_then(Value::as_f64).unwrap_or(0.0);
    Money::usd(format!("{amount:.2}"))
}

/// Same, for the endpoints that report **cents**.
///
/// `/Ledger` is the outlier: it returns integer cents (`25000`) while
/// `/Payment`, `/Charge` and `/account` return dollars (`250`) for the very
/// same transaction. Mixing the two up inflates a ledger by 100×, so the scale
/// is part of each call site rather than something inferred. See `docs/api.md`.
pub fn money_cents(v: Option<&Value>) -> Money {
    Money::from_cents(cents_of(v).unwrap_or(0))
}

/// Read a cents field as an integer. The portal sends whole numbers, but a
/// float is accepted rather than dropped — a `250.0` would otherwise silently
/// become `0.00`.
fn cents_of(v: Option<&Value>) -> Option<i64> {
    let v = v?;
    v.as_i64().or_else(|| v.as_f64().map(|f| f.round() as i64))
}

/// A cents-scaled money field the portal may legitimately leave null — a
/// scheduled payment whose rule is "draft the full balance" carries no fixed
/// amount. Null stays null instead of becoming a misleading `$0.00`.
pub fn money_opt_cents(v: Option<&Value>) -> Value {
    match cents_of(v) {
        Some(cents) => serde_json::to_value(Money::from_cents(cents)).unwrap_or(Value::Null),
        None => Value::Null,
    }
}

/// How many records to request per page. The portal defaults to **10**, which
/// silently truncates anything longer — a board directory, a year of ledger.
/// Every paged read goes through [`fetch_all`] rather than relying on that.
const PAGE_SIZE: u32 = 200;

/// Walk a paged endpoint to exhaustion, returning every record.
///
/// Bounded by the `lastPage` the portal reports, with a hard stop so a portal
/// bug can't spin forever. If that stop is ever hit the shortfall is reported
/// on stderr — a truncated list that looks complete is worse than a slow one.
pub fn fetch_all(
    ctx: &Ctx,
    service: Service,
    path: &str,
) -> Result<(Vec<Value>, Option<u64>), CliError> {
    const MAX_PAGES: u32 = 100;
    let client = ctx.client()?;
    let mut all = Vec::new();
    let mut total = None;
    let mut page = 1;
    loop {
        let body = client.get(
            service,
            path,
            &[
                ("page", page.to_string()),
                ("pageSize", PAGE_SIZE.to_string()),
            ],
        )?;
        let (items, reported) = members(&body);
        let last_page = body.get("lastPage").and_then(Value::as_u64).unwrap_or(1);
        if total.is_none() {
            total = reported;
        }
        let empty = items.is_empty();
        all.extend(items);
        if empty || page as u64 >= last_page {
            break;
        }
        if page >= MAX_PAGES {
            if !ctx.common.quiet {
                eprintln!(
                    "warning: stopped after {MAX_PAGES} pages ({} of {} records) — \
                     narrow the range to see the rest",
                    all.len(),
                    total.map(|t| t.to_string()).unwrap_or_else(|| "?".into())
                );
            }
            break;
        }
        page += 1;
    }
    Ok((all, total))
}

/// Records under the portal's paged envelope (`{page, lastPage, totalItems,
/// member}`), plus the reported total.
pub fn members(body: &Value) -> (Vec<Value>, Option<u64>) {
    let items = body
        .get("member")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    (items, body.get("totalItems").and_then(Value::as_u64))
}

/// Apply `--limit` to an already-filtered list.
pub fn limited(mut items: Vec<Value>, limit: Option<u32>) -> Vec<Value> {
    if let Some(n) = limit {
        items.truncate(n as usize);
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn table_view_selects_and_skips_missing() {
        let items = vec![json!({ "a": 1, "b": 2, "c": 3 }), json!({ "a": 4 })];
        let rows = table_view(&items, &["a", "c"]);
        assert_eq!(rows[0], json!({ "a": 1, "c": 3 }));
        // Absent columns are omitted rather than nulled (SPEC: omit-don't-null).
        assert_eq!(rows[1], json!({ "a": 4 }));
    }

    #[test]
    fn money_formats_to_two_decimals() {
        assert_eq!(money(Some(&json!(207))).amount, "207.00");
        assert_eq!(money(Some(&json!(207.5))).amount, "207.50");
        assert_eq!(money(Some(&json!(-12.345))).amount, "-12.35");
        assert_eq!(money(Some(&json!(0))).currency, "USD");
    }

    #[test]
    fn money_tolerates_missing_and_wrong_types() {
        assert_eq!(money(None).amount, "0.00");
        assert_eq!(money(Some(&json!("nope"))).amount, "0.00");
        assert_eq!(money(Some(&Value::Null)).amount, "0.00");
    }

    /// The ledger's 25000 and the payment list's 250 are the same $250.00.
    #[test]
    fn cents_and_dollars_agree_on_the_same_transaction() {
        assert_eq!(money_cents(Some(&json!(25000))).amount, "250.00");
        assert_eq!(money(Some(&json!(250))).amount, "250.00");
        assert_eq!(money_cents(Some(&json!(-25000))).amount, "-250.00");
        assert_eq!(money_cents(Some(&json!(0))).amount, "0.00");
        assert_eq!(money_cents(None).amount, "0.00");
    }

    /// A scheduled payment with no fixed amount must stay null: `$0.00` would
    /// read as "drafts nothing" when it actually drafts the full balance.
    #[test]
    fn optional_money_keeps_null_distinct_from_zero() {
        assert_eq!(money_opt_cents(None), Value::Null);
        assert_eq!(money_opt_cents(Some(&Value::Null)), Value::Null);
        assert_eq!(money_opt_cents(Some(&json!(0)))["amount"], json!("0.00"));
        assert_eq!(
            money_opt_cents(Some(&json!(25000)))["amount"],
            json!("250.00")
        );
    }

    #[test]
    fn members_reads_the_paged_envelope() {
        let body = json!({ "page": 1, "lastPage": 2, "totalItems": 7, "member": [{ "id": 1 }] });
        let (items, total) = members(&body);
        assert_eq!(items.len(), 1);
        assert_eq!(total, Some(7));
    }

    #[test]
    fn members_tolerates_a_missing_envelope() {
        let (items, total) = members(&json!({}));
        assert!(items.is_empty());
        assert_eq!(total, None);
    }

    #[test]
    fn limit_truncates_only_when_asked() {
        let items = vec![json!(1), json!(2), json!(3)];
        assert_eq!(limited(items.clone(), None).len(), 3);
        assert_eq!(limited(items.clone(), Some(2)).len(), 2);
        assert_eq!(limited(items, Some(99)).len(), 3);
    }
}
