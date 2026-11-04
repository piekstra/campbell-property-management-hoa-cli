//! `cpmfl mcp`: read-only tools for Claude, over the reads the commands make.
//!
//! The balance, bills and payments tools are the shared `utility/v1` set
//! (`pk_cli_utility::mcp`), over [`Source`]. Requests and documents are this
//! portal's own. Every read goes through [`Reads`], which offers no way to send
//! anything but a GET, and tests put fixtures behind it, so nothing here needs a
//! login to test.
//!
//! The server reads the cached token and nothing else. It never sends the
//! password and never writes the keychain: a lapsed token is an auth error
//! naming `cpmfl auth login`, so a stale password can never be resent by a
//! background server until the account locks. The config and the token are read
//! on each call, so a running server sees the sign-in the user does next.
//!
//! The commands' mappers are best-effort, for a person who can see the raw
//! payload. A tool's answer goes to a model as fact, so an answer the portal did
//! not give (a balance no field states, a list its answer no longer holds) is an
//! upstream error here, never 0.00 or an empty list.
//!
//! Document rows carry a short-lived SAS URL that needs no sign-in. No tool
//! returns it. The board directory is left out: it names other people.

use std::collections::HashMap;
use std::sync::Arc;

use pk_cli_config::ConfigStore;
use pk_cli_core::{dates, CliError, Money};
use pk_cli_mcp::{
    parse_args, ListArgs, Order, Page, PageRequest, PositionKey, PositionRequest, Server, ToolDef,
    ToolError, ToolOutput, Toolset,
};
use pk_cli_secrets::{CredentialStore, KeychainRead, SecretResolver, SecretSpec};
use pk_cli_utility::mcp::{UtilityAccount, UtilitySource, UtilityTools};
use pk_cli_utility::{Payment, Statement, UsagePeriod, UtilitySummary};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::Portal;
use crate::commands::{self, cents_of};
use crate::config::{Config, Service, TOKEN_ACCOUNT};
use crate::dates::iso_date;

/// The token's environment variable, read before the keychain.
const TOKEN_ENV: &str = "CPMFL_TOKEN";

/// The tools `cpmfl mcp` serves. Nothing here reads the config or the keychain:
/// a client lists tools before anyone has signed in.
pub fn tools() -> CampbellTools<LivePortal> {
    with_reads(LivePortal, std::env::var_os(TOKEN_ENV).is_some())
}

fn with_reads<R: Reads>(reads: R, token_in_env: bool) -> CampbellTools<R> {
    let mut server = Server::new(
        "cpmfl",
        env!("CARGO_PKG_VERSION"),
        "Campbell HOA",
        "campbell",
    )
    .instructions(
        "Access here is read-only: no tool can make a payment, change autopay, submit a request \
         or change anything at the portal. A balance's due_date is the portal's next charge \
         date. Bills are the ledger's posted charges plus any charge assessed but not yet \
         posted; voided ones are left out. A login with several accounts cannot tell whose an \
         unposted charge is, so its bills hold posted charges only. The portal's session expires \
         and a tool cannot renew it.",
    );
    // The keychain is not read while the variable is set, so `cpmfl auth login`
    // would store a token this server never sees.
    if token_in_env {
        server = server.hosted(
            "through the open-source `cpmfl` tool, on a token from the settings of the program \
             that started this server",
            "ask the user to replace or unset $CPMFL_TOKEN where it is set, then restart this \
             server",
        );
    }
    let reads = Arc::new(reads);
    CampbellTools {
        utility: UtilityTools::new(
            server,
            Source {
                reads: reads.clone(),
            },
        )
        .without_usage(),
        reads,
    }
}

/// The portal reads the tools need, as the portal answers them. All of them are
/// GETs; this seam is what keeps every write out of reach of a tool.
pub trait Reads {
    /// `GET {pay}/account`: one record per ledger on the login.
    fn accounts(&self) -> Result<Value, CliError>;
    /// Every row of `GET {pay}/Ledger`.
    fn ledger(&self) -> Result<Vec<Value>, CliError>;
    /// Every row of `GET {pay}/Payment`.
    fn payments(&self) -> Result<Vec<Value>, CliError>;
    /// `GET {pay}/Charge`.
    fn charges(&self) -> Result<Value, CliError>;
    /// Every row of `GET {actionitems}/HomeownerRequests`.
    fn requests(&self) -> Result<Vec<Value>, CliError>;
    /// The association the config names, if any.
    fn configured_association(&self) -> Result<Option<u64>, CliError>;
    /// `GET {associations}/associations/{a}/Directories`, at the root or in a folder.
    fn library(&self, association: u64, folder: Option<u64>) -> Result<Value, CliError>;
    /// One page of `GET {associations}/associations/{a}/Documents/Search`.
    fn search(&self, association: u64, term: &str, page: u32) -> Result<Value, CliError>;
}

/// [`Reads`] over the portal, reading the config and the token on each call.
pub struct LivePortal;

impl LivePortal {
    fn config() -> Result<Config, CliError> {
        ConfigStore::new("cpmfl").load().map_err(Self::bad_config)
    }

    /// A command's usage error, for a bad config file. No tool argument can fix it.
    fn bad_config(e: CliError) -> CliError {
        match e {
            CliError::Usage(message) => CliError::Other(message),
            other => other,
        }
    }

    fn portal() -> Result<Portal, CliError> {
        let cfg = Self::config()?;
        // Before the keychain, so a refusal raises no dialog.
        cfg.require_secure_urls().map_err(Self::bad_config)?;
        let creds = CredentialStore::for_binary("cpmfl");
        Portal::with_live_token(&cfg, resolve_token(&creds, TOKEN_ENV)?)
    }

    fn get(service: Service, path: &str, query: &[(&str, String)]) -> Result<Value, CliError> {
        Self::portal()?
            .get(service, path, query)
            .map_err(portal_error)
    }

    fn all(service: Service, path: &str, what: &str) -> Result<Vec<Value>, CliError> {
        let walked = commands::walk(&Self::portal()?, service, path).map_err(portal_error)?;
        if !walked.shaped {
            return Err(shape_error(what));
        }
        if walked.cut_short {
            return Err(CliError::Upstream(format!(
                "the portal's {what} ran past the most pages this tool reads, so it is not \
                 reported here"
            )));
        }
        Ok(walked.items)
    }
}

impl Reads for LivePortal {
    fn accounts(&self) -> Result<Value, CliError> {
        Self::get(Service::Pay, "/account", &[])
    }

    fn ledger(&self) -> Result<Vec<Value>, CliError> {
        Self::all(Service::Pay, "/Ledger", "ledger")
    }

    fn payments(&self) -> Result<Vec<Value>, CliError> {
        Self::all(Service::Pay, "/Payment", "payment list")
    }

    fn charges(&self) -> Result<Value, CliError> {
        Self::get(Service::Pay, "/Charge", &[])
    }

    fn requests(&self) -> Result<Vec<Value>, CliError> {
        Self::all(Service::ActionItems, "/HomeownerRequests", "request list")
    }

    fn configured_association(&self) -> Result<Option<u64>, CliError> {
        Ok(Self::config()?.association_id)
    }

    fn library(&self, association: u64, folder: Option<u64>) -> Result<Value, CliError> {
        // `parentId` is the one spelling: the portal answers any other with the
        // root listing and a 200.
        let query: Vec<(&str, String)> = folder
            .map(|id| vec![("parentId", id.to_string())])
            .unwrap_or_default();
        Self::get(
            Service::Associations,
            &format!("/associations/{association}/Directories"),
            &query,
        )
    }

    fn search(&self, association: u64, term: &str, page: u32) -> Result<Value, CliError> {
        Self::get(
            Service::Associations,
            &format!("/associations/{association}/Documents/Search"),
            &[("search", term.to_string()), ("page", page.to_string())],
        )
    }
}

/// The family's secret precedence: the variable, then the keychain.
fn resolve_token(
    keychain: &impl KeychainRead,
    env: &str,
) -> Result<pk_cli_secrets::Secret, CliError> {
    let spec = SecretSpec::new(TOKEN_ACCOUNT)
        .env(env)
        .remedy("`cpmfl auth login`");
    // A variable set but empty is a usage error on a command line. Here no tool
    // argument can fix it, and the model would be told to try another.
    let found = SecretResolver::new(keychain)
        .resolve(&spec)
        .map_err(|e| match e {
            CliError::Usage(message) => CliError::Auth(message),
            other => other,
        })?;
    found
        .map(|r| r.secret)
        .ok_or_else(|| CliError::Auth("not signed in to the portal".into()))
}

/// A 400 is the portal's complaint about a request the tool built, not about
/// anything the model passed, so the usage error it maps to would send the
/// model after an argument that is not wrong.
fn portal_error(e: CliError) -> CliError {
    match e {
        CliError::Usage(message) => CliError::Upstream(format!("the portal said: {message}")),
        other => other,
    }
}

fn shape_error(what: &str) -> CliError {
    CliError::Upstream(format!(
        "the portal's {what} did not have the expected shape, so it is not reported here"
    ))
}

/// What identifies a ledger: the sub-coded number when there is one, since one
/// account number may span several ledgers.
fn ledger_id(row: &Value, number_key: &str) -> Option<String> {
    ["accountWithSubCode", number_key]
        .iter()
        .find_map(|key| row.get(*key).and_then(Value::as_str))
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

fn date_of(row: &Value, key: &str, what: &str) -> Result<String, CliError> {
    row.get(key)
        .and_then(Value::as_str)
        .and_then(iso_date)
        .ok_or_else(|| shape_error(what))
}

/// A dollars field, refusing what is not a number rather than reading it as zero.
fn dollars(row: &Value, key: &str, what: &str) -> Result<Money, CliError> {
    let field = row.get(key).filter(|v| v.is_number());
    match field {
        Some(_) => Ok(commands::money(field)),
        None => Err(shape_error(what)),
    }
}

/// Newest first, by date; rows the portal dated alike keep the portal's order.
fn newest_first<T>(rows: &mut [T], date: impl Fn(&T) -> Option<&str>) {
    rows.sort_by(|a, b| date(b).cmp(&date(a)));
}

/// A bill's id. A pending charge and the ledger row it posts as have different ids at the portal, and
/// a position holds this one, so it is built from what makes the two one bill: its day and amount.
/// `statements` numbers a second of one day and amount.
fn bill_id(day: &str, amount: &Money) -> String {
    format!("{day}:{}", amount.amount)
}

/// [`UtilitySource`] over [`Reads`]: the shared tools' balance, bills and payments.
pub struct Source<R> {
    reads: Arc<R>,
}

impl<R: Reads> Source<R> {
    fn records(&self) -> Result<Vec<Value>, CliError> {
        records_of(self.reads.as_ref())
    }

    /// The record for `account`, or with none named, the first: the one the
    /// commands read with no flag.
    fn record<'a>(
        records: &'a [Value],
        account: Option<&str>,
    ) -> Result<(&'a Value, String), CliError> {
        let found = match account {
            None => records.first(),
            Some(id) => records
                .iter()
                .find(|r| ledger_id(r, "accountNumber").as_deref() == Some(id)),
        }
        .ok_or_else(|| CliError::NotFound("no such account on this login".into()))?;
        let id = ledger_id(found, "accountNumber").ok_or_else(|| shape_error("account record"))?;
        Ok((found, id))
    }
}

/// `GET /account`, which answers a list even for one ledger.
fn records_of<R: Reads + ?Sized>(reads: &R) -> Result<Vec<Value>, CliError> {
    match reads.accounts()? {
        Value::Array(records) if records.is_empty() => Err(CliError::NotFound(
            "this login has no association account".into(),
        )),
        Value::Array(records) => Ok(records),
        _ => Err(shape_error("account record")),
    }
}

/// A ledger the portal is withholding reads as empty. That is not "nothing was
/// charged", so say what the portal said instead.
fn withheld(record: &Value, rows_found: bool, what: &str) -> Result<(), CliError> {
    if rows_found || record.get("blockLedger").and_then(Value::as_bool) != Some(true) {
        return Ok(());
    }
    let reason = record
        .get("blockLedgerMessage")
        .and_then(Value::as_str)
        .filter(|m| !m.is_empty())
        .unwrap_or("it gave no reason");
    Err(CliError::Upstream(format!(
        "the portal is withholding this account's {what}: {reason}"
    )))
}

impl<R: Reads> UtilitySource for Source<R> {
    async fn accounts(&self) -> Result<Vec<UtilityAccount>, CliError> {
        self.records()?
            .iter()
            .map(|r| {
                Ok(UtilityAccount {
                    id: ledger_id(r, "accountNumber")
                        .ok_or_else(|| shape_error("account record"))?,
                    name: r
                        .pointer("/propertyAddress/address1")
                        .and_then(Value::as_str)
                        .filter(|a| !a.is_empty())
                        .map(str::to_string),
                })
            })
            .collect()
    }

    async fn summary(&self, account: Option<&str>) -> Result<UtilitySummary, CliError> {
        let records = self.records()?;
        let (record, id) = Self::record(&records, account)?;
        let mut summary = UtilitySummary::new(dollars(record, "balance", "account record")?);
        summary.due_date = record
            .get("nextChargeDate")
            .and_then(Value::as_str)
            .and_then(iso_date);
        summary.account = Some(id);
        summary.autopay = record.get("autoDraftEnrolled").and_then(Value::as_bool);
        Ok(summary)
    }

    async fn statements(&self, account: Option<&str>) -> Result<Vec<Statement>, CliError> {
        let records = self.records()?;
        let (record, id) = Self::record(&records, account)?;
        let rows = self.reads.ledger()?;
        let mut bills = Vec::new();
        // Any row of this account's, even one not listed here: a ledger of only
        // payments is not a withheld one. Another account's rows say nothing.
        let mut any_of_its_rows = false;
        for row in &rows {
            let kind = row
                .get("transactionType")
                .and_then(Value::as_str)
                .ok_or_else(|| shape_error("ledger"))?;
            if ledger_id(row, "accountNumber").ok_or_else(|| shape_error("ledger"))? != id {
                continue;
            }
            any_of_its_rows = true;
            if !kind.eq_ignore_ascii_case("charge")
                || row.get("isVoided").and_then(Value::as_bool) == Some(true)
            {
                continue;
            }
            // Cents on this endpoint, unlike every other money field.
            let cents = cents_of(row.get("amount")).ok_or_else(|| shape_error("ledger"))?;
            let date = date_of(row, "ledgerDate", "ledger")?;
            let amount = Money::from_cents(cents);
            bills.push(Statement {
                id: bill_id(&date, &amount),
                date: Some(date),
                amount,
                due_date: None,
                paid: None,
            });
        }
        withheld(record, any_of_its_rows, "ledger")?;
        // `/Charge` rows name no account, so with several there is no telling
        // whose they are.
        if records.len() == 1 {
            let charges = self.reads.charges()?;
            let charges = charges
                .as_array()
                .ok_or_else(|| shape_error("charge list"))?;
            for row in charges {
                let due = date_of(row, "dueDate", "charge list")?;
                let amount = dollars(row, "amount", "charge list")?;
                // A charge the ledger has posted is not a second bill.
                let posted = bills.iter().any(|b| {
                    b.date.as_deref() == Some(due.as_str()) && b.amount.amount == amount.amount
                });
                if !posted {
                    bills.push(Statement {
                        id: bill_id(&due, &amount),
                        date: None,
                        amount,
                        due_date: Some(due),
                        paid: Some(false),
                    });
                }
            }
        }
        newest_first(&mut bills, |b| b.date.as_deref().or(b.due_date.as_deref()));
        // Two of one day and amount are two bills, and a reader of `id` has to be able to tell.
        let mut repeats: HashMap<String, u32> = HashMap::new();
        for bill in &mut bills {
            let nth = repeats.entry(bill.id.clone()).or_default();
            *nth += 1;
            if *nth > 1 {
                bill.id = format!("{}:{nth}", bill.id);
            }
        }
        Ok(bills)
    }

    async fn payments(
        &self,
        account: Option<&str>,
        _since: Option<&str>,
    ) -> Result<Vec<Payment>, CliError> {
        let records = self.records()?;
        let (record, id) = Self::record(&records, account)?;
        let rows = self.reads.payments()?;
        let mut payments = Vec::new();
        for row in &rows {
            if ledger_id(row, "account").ok_or_else(|| shape_error("payment list"))? != id {
                continue;
            }
            let pending = row
                .get("pending")
                .and_then(Value::as_bool)
                .ok_or_else(|| shape_error("payment list"))?;
            payments.push(Payment {
                date: date_of(row, "date", "payment list")?,
                amount: dollars(row, "amount", "payment list")?,
                method: row
                    .get("paymentType")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                status: Some(if pending { "pending" } else { "posted" }.into()),
                confirmation: row.get("id").and_then(Value::as_str).map(str::to_string),
            });
        }
        withheld(record, !payments.is_empty(), "payments")?;
        newest_first(&mut payments, |p| Some(p.date.as_str()));
        Ok(payments)
    }

    /// Never called: the toolset has no usage tool.
    async fn usage(&self, _account: Option<&str>) -> Result<Vec<UsagePeriod>, CliError> {
        Ok(Vec::new())
    }
}

/// `campbell_requests`' arguments.
#[derive(Debug, Deserialize, JsonSchema)]
struct RequestArgs {
    /// Earliest date submitted, YYYY-MM-DD.
    #[serde(default)]
    since: Option<String>,
    /// Latest date submitted, YYYY-MM-DD.
    #[serde(default)]
    until: Option<String>,
    /// Also list requests the association has closed (default: open ones only).
    #[serde(default)]
    include_closed: Option<bool>,
    #[serde(flatten)]
    list: ListArgs,
}

/// The filters a requests cursor is bound to.
#[derive(Serialize)]
struct RequestFilter<'a> {
    since: Option<&'a str>,
    until: Option<&'a str>,
    include_closed: bool,
}

/// One homeowner request.
#[derive(Debug, Serialize, JsonSchema)]
struct RequestItem {
    submitted: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    closed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
}

/// `campbell_documents`' arguments.
#[derive(Debug, Deserialize, JsonSchema)]
struct DocumentArgs {
    /// Words to find in document names, across the whole library. Omit to list
    /// a folder.
    #[serde(default)]
    query: Option<String>,
    /// A folder_id from an earlier call, to list that folder; omit for the top
    /// of the library. Not with query.
    #[serde(default)]
    folder: Option<u64>,
    #[serde(flatten)]
    list: ListArgs,
}

/// The filters a documents cursor is bound to.
#[derive(Serialize)]
struct DocumentFilter<'a> {
    query: Option<&'a str>,
    folder: Option<u64>,
}

/// One row of the document library, without the link its row carries.
#[derive(Debug, Serialize, JsonSchema)]
struct DocumentItem {
    name: String,
    /// `file` or `folder`.
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    /// Pass as `folder` to list it. Folders only.
    #[serde(skip_serializing_if = "Option::is_none")]
    folder_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
}

/// `campbell_documents`' result: a page, and whose library it is.
#[derive(Debug, Serialize, JsonSchema)]
struct DocumentsOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    association: Option<String>,
    #[serde(flatten)]
    page: Page<DocumentItem>,
}

/// The toolset `cpmfl mcp` serves: the shared set, and this portal's own two.
pub struct CampbellTools<R> {
    utility: UtilityTools<Source<R>>,
    reads: Arc<R>,
}

fn text_of(row: &Value, key: &str) -> Option<String> {
    row.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `since` and `until` in canonical form, so they compare as text.
fn bounds(
    since: Option<&str>,
    until: Option<&str>,
) -> Result<(Option<String>, Option<String>), ToolError> {
    let bound = |name: &str, value: Option<&str>| {
        value
            .filter(|v| !v.trim().is_empty())
            .map(|d| {
                dates::parse_iso(d)
                    .map(dates::fmt_iso)
                    .map_err(|_| ToolError::usage(format!("{name} must be YYYY-MM-DD; got `{d}`")))
            })
            .transpose()
    };
    let since = bound("since", since)?;
    let until = bound("until", until)?;
    if let (Some(s), Some(u)) = (&since, &until) {
        if s > u {
            return Err(ToolError::usage(format!("since {s} is after until {u}")));
        }
    }
    Ok((since, until))
}

impl<R: Reads> CampbellTools<R> {
    fn requests(&self, args: Value) -> Result<ToolOutput, ToolError> {
        let args: RequestArgs = parse_args(args)?;
        let (since, until) = bounds(args.since.as_deref(), args.until.as_deref())?;
        let include_closed = args.include_closed.unwrap_or(false);
        let page = PositionRequest::new(
            &args.list,
            &RequestFilter {
                since: since.as_deref(),
                until: until.as_deref(),
                include_closed,
            },
        )?;
        let detailed = args.list.format().is_detailed();
        let mut items = Vec::new();
        for row in self.reads.requests()? {
            let submitted = date_of(&row, "submitted", "request list")?;
            let closed = row
                .get("closed")
                .and_then(Value::as_bool)
                .ok_or_else(|| shape_error("request list"))?;
            if (closed && !include_closed)
                || since.as_deref().is_some_and(|s| submitted.as_str() < s)
                || until.as_deref().is_some_and(|u| submitted.as_str() > u)
            {
                continue;
            }
            let id = row
                .get("id")
                .and_then(Value::as_u64)
                .ok_or_else(|| shape_error("request list"))?;
            let key = PositionKey::new(submitted.as_str(), &id.to_string());
            items.push((
                key,
                RequestItem {
                    submitted,
                    kind: text_of(&row, "type"),
                    subject: text_of(&row, "subject"),
                    status: text_of(&row, "status"),
                    closed,
                    id: detailed.then_some(id),
                },
            ));
        }
        ToolOutput::new(&page.page(items, Order::Descending, "since/until")?)
    }

    fn documents(&self, args: Value) -> Result<ToolOutput, ToolError> {
        let args: DocumentArgs = parse_args(args)?;
        let query = args
            .query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty());
        if query.is_some() && args.folder.is_some() {
            return Err(ToolError::usage(
                "query searches the whole library and folder lists one folder: pass one of them",
            ));
        }
        let page = PageRequest::new(
            &args.list,
            &DocumentFilter {
                query,
                folder: args.folder,
            },
        )?;
        let detailed = args.list.format().is_detailed();

        let records = records_of(self.reads.as_ref())?;
        let id_of = |r: &Value| r.get("associationId").and_then(Value::as_u64);
        let association = self
            .reads
            .configured_association()?
            .or_else(|| records.first().and_then(id_of))
            .ok_or_else(|| shape_error("account record"))?;
        let name = records
            .iter()
            .find(|r| id_of(r) == Some(association))
            .and_then(|r| text_of(r, "associationName"));

        let (rows, more) = match query {
            Some(term) => self.search_rows(association, term, page.wanted())?,
            None => {
                let body = self.reads.library(association, args.folder)?;
                let rows = body
                    .get("items")
                    .and_then(Value::as_array)
                    .ok_or_else(|| shape_error("document library"))?;
                (rows.clone(), false)
            }
        };
        let items = rows
            .iter()
            .map(|row| document_item(row, detailed))
            .collect::<Result<Vec<_>, _>>()?;
        ToolOutput::new(&DocumentsOut {
            association: name,
            page: page.page_with_more(items, more, "query or folder")?,
        })
    }

    /// Search's pages, until `wanted` rows or the last: rows, and whether more
    /// exist.
    fn search_rows(
        &self,
        association: u64,
        term: &str,
        wanted: usize,
    ) -> Result<(Vec<Value>, bool), CliError> {
        let mut rows: Vec<Value> = Vec::new();
        let mut page = 1;
        loop {
            let body = self.reads.search(association, term, page)?;
            let found = body
                .get("documents")
                .and_then(Value::as_array)
                .ok_or_else(|| shape_error("document search"))?;
            rows.extend(found.iter().cloned());
            if found.is_empty() {
                return Ok((rows, false));
            }
            // Without it, page 1 would read as the last page and a truncated list
            // as a complete one.
            let last = body
                .get("totalPages")
                .and_then(Value::as_u64)
                .ok_or_else(|| shape_error("document search"))?;
            if u64::from(page) >= last {
                return Ok((rows, false));
            }
            if rows.len() >= wanted {
                return Ok((rows, true));
            }
            page += 1;
        }
    }
}

fn document_item(row: &Value, detailed: bool) -> Result<DocumentItem, CliError> {
    let name = text_of(row, "name").ok_or_else(|| shape_error("document library"))?;
    let kind = text_of(row, "type").ok_or_else(|| shape_error("document library"))?;
    let id = row.get("id").and_then(Value::as_u64);
    Ok(DocumentItem {
        folder_id: id.filter(|_| kind == "folder"),
        id: id.filter(|_| detailed),
        last_modified: row
            .get("lastModified")
            .and_then(Value::as_str)
            .and_then(iso_date),
        path: text_of(row, "fullPath"),
        name,
        kind,
    })
}

/// Five tools have to fit the 8,000-character budget (they ride along in every
/// prompt), and the shared argument descriptions take a quarter of it.
fn tighten(tool: &mut ToolDef) {
    for (arg, text) in [
        (
            "account",
            "Its number, or a unique part of its address. Omit for the default.",
        ),
        (
            "cursor",
            "A result's next_cursor, to continue the same query.",
        ),
        (
            "response_format",
            "`detailed` adds ids and provider detail.",
        ),
    ] {
        if let Some(description) = tool
            .input_schema
            .pointer_mut(&format!("/properties/{arg}/description"))
        {
            *description = Value::String(text.into());
        }
    }
}

impl<R: Reads> Toolset for CampbellTools<R> {
    fn server(&self) -> Server {
        self.utility.server()
    }

    fn tools(&self) -> Vec<ToolDef> {
        let mut tools = self.utility.tools();
        tools.push(
            ToolDef::read(
                "campbell_requests",
                "Campbell HOA requests",
                "Lists the homeowner requests the user submitted (work orders, architectural \
                 applications, questions), newest day first. Use it to see where a request stands. \
                 Items carry submitted, kind, subject, status and closed (detailed adds id). \
                 Open ones only unless include_closed.",
            )
            .input::<RequestArgs>()
            .output::<Page<RequestItem>>(),
        );
        tools.push(
            ToolDef::read(
                "campbell_documents",
                "Campbell HOA documents",
                "Finds the association's published documents by name, or lists a folder. Use it \
                 for governing documents, budgets, minutes and financial statements. Returns \
                 association and items with name, kind, last_modified, path and, on folders, \
                 folder_id (detailed adds id). It gives no contents or links.",
            )
            .input::<DocumentArgs>()
            .output::<DocumentsOut>(),
        );
        tools.iter_mut().for_each(tighten);
        tools
    }

    async fn call(&self, name: &str, args: Value) -> Result<ToolOutput, ToolError> {
        match name {
            "campbell_requests" => self.requests(args),
            "campbell_documents" => self.documents(args),
            _ => self.utility.call(name, args).await,
        }
    }
}

#[cfg(test)]
mod tests;
