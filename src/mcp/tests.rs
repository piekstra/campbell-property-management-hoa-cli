//! The `cpmfl mcp` tools, offline: `tests/fixtures/` behind [`Reads`], checked by
//! cli-common's conformance helper. `tests/mcp_stdio.rs`, `tests/mcp_http.rs` and
//! `tests/mcp_readonly.rs` drive the real binary.

use std::cell::Cell;
use std::rc::Rc;

use pk_cli_mcp::conformance::{assert_conforms, assert_fits, call, definitions_size};
use pk_cli_secrets::Secret;
use serde_json::json;

use super::*;

const TOOLS: [&str; 5] = [
    "campbell_balance",
    "campbell_bills",
    "campbell_payments",
    "campbell_requests",
    "campbell_documents",
];

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn members(name: &str) -> Vec<Value> {
    fixture(name)["member"].as_array().unwrap().clone()
}

struct Fixtures {
    accounts: Value,
    ledger: Vec<Value>,
    payments: Vec<Value>,
    charges: Value,
    requests: Vec<Value>,
    library: Value,
    search: Value,
    configured: Option<u64>,
    reads: Rc<Cell<u32>>,
    failure: Option<fn() -> CliError>,
}

impl Fixtures {
    fn new() -> Self {
        Self {
            accounts: fixture("account.json"),
            ledger: members("ledger.json"),
            payments: members("payments.json"),
            charges: fixture("charges.json"),
            requests: members("homeowner_requests.json"),
            library: fixture("directories.json"),
            search: fixture("document_search.json"),
            configured: None,
            reads: Rc::default(),
            failure: None,
        }
    }

    fn read<T: Clone>(&self, value: &T) -> Result<T, CliError> {
        self.reads.set(self.reads.get() + 1);
        match self.failure {
            Some(failure) => Err(failure()),
            None => Ok(value.clone()),
        }
    }
}

impl Reads for Fixtures {
    fn accounts(&self) -> Result<Value, CliError> {
        self.read(&self.accounts)
    }
    fn ledger(&self) -> Result<Vec<Value>, CliError> {
        self.read(&self.ledger)
    }
    fn payments(&self) -> Result<Vec<Value>, CliError> {
        self.read(&self.payments)
    }
    fn charges(&self) -> Result<Value, CliError> {
        self.read(&self.charges)
    }
    fn requests(&self) -> Result<Vec<Value>, CliError> {
        self.read(&self.requests)
    }
    fn configured_association(&self) -> Result<Option<u64>, CliError> {
        Ok(self.configured)
    }
    fn library(&self, _association: u64, _folder: Option<u64>) -> Result<Value, CliError> {
        self.read(&self.library)
    }
    fn search(&self, _association: u64, _term: &str, _page: u32) -> Result<Value, CliError> {
        self.read(&self.search)
    }
}

fn tools_over(fixtures: Fixtures) -> CampbellTools<Fixtures> {
    with_reads(fixtures, false)
}

fn tools() -> CampbellTools<Fixtures> {
    tools_over(Fixtures::new())
}

fn ok(tools: &CampbellTools<Fixtures>, name: &str, args: Value) -> Value {
    let result = call(tools, name, args);
    assert_fits(tools, name, &result).clone()
}

fn error(tools: &CampbellTools<Fixtures>, name: &str, args: Value) -> String {
    let result = call(tools, name, args);
    assert!(result.is_error(), "{name}: {}", result.text());
    result.text().to_string()
}

#[test]
fn the_tools_conform_and_fit_the_definition_budget() {
    let tools = tools();
    assert_conforms(&tools);
    let size = definitions_size(&tools);
    assert!(size < 8_000, "{size} characters");
    let names: Vec<String> = tools.tools().into_iter().map(|t| t.name).collect();
    assert_eq!(names, TOOLS);
}

/// Listing reads nothing, so it needs no credential.
#[test]
fn listing_the_tools_reads_nothing() {
    let fixtures = Fixtures::new();
    let reads = fixtures.reads.clone();
    let tools = tools_over(fixtures);
    let _ = tools.tools();
    let _ = pk_cli_mcp::instructions(&tools);
    assert_eq!(reads.get(), 0);
}

/// Every tool is a read: annotated so, and with no approval prompt, which only a
/// write carries. Each CLI verb the portal could act on is refused by name, and
/// `tests/mcp_readonly.rs` shows that what the tools send is GETs alone.
#[test]
fn no_tool_definition_reaches_a_mutating_command() {
    const MUTATING: [&str; 14] = [
        "pay", "autopay", "enroll", "submit", "file", "edit", "update", "delete", "cancel",
        "approve", "reserve", "book", "upload", "login",
    ];
    for tool in tools().tools() {
        assert!(tool.annotations.read_only, "{}", tool.name);
        assert!(!tool.annotations.destructive, "{}", tool.name);
        assert!(!tool.requires_user_interaction(), "{}", tool.name);
        let verb = tool.name.strip_prefix("campbell_").unwrap();
        assert!(
            !MUTATING.contains(&verb),
            "{} is named for a mutating command",
            tool.name
        );
    }
}

#[test]
fn balance_reads_the_account_card() {
    let got = ok(&tools(), "campbell_balance", json!({}));
    assert_eq!(got["balance"], "0.00");
    assert_eq!(got["due_date"], "2026-10-01");
    assert_eq!(got["autopay"], true);
    assert_eq!(got["account"], "100 Sample St");
    assert!(got.get("account_id").is_none(), "{got}");

    let got = ok(
        &tools(),
        "campbell_balance",
        json!({"response_format": "detailed"}),
    );
    assert_eq!(got["account_id"], "SMP100000");
}

#[test]
fn a_balance_no_field_states_is_an_error_not_zero() {
    let mut fixtures = Fixtures::new();
    fixtures.accounts[0]
        .as_object_mut()
        .unwrap()
        .remove("balance");
    let text = error(&tools_over(fixtures), "campbell_balance", json!({}));
    assert!(text.starts_with("upstream:"), "{text}");

    let mut fixtures = Fixtures::new();
    fixtures.accounts = json!({"member": []});
    let text = error(&tools_over(fixtures), "campbell_balance", json!({}));
    assert!(text.starts_with("upstream:"), "{text}");
}

#[test]
fn a_login_with_no_account_says_so() {
    let mut fixtures = Fixtures::new();
    fixtures.accounts = json!([]);
    let text = error(&tools_over(fixtures), "campbell_bills", json!({}));
    assert!(text.starts_with("not_found:"), "{text}");
}

fn second_account(fixtures: &mut Fixtures) {
    let mut other = fixtures.accounts[0].clone();
    other["accountNumber"] = json!("SMP200000");
    other["accountWithSubCode"] = json!("SMP200000");
    other["balance"] = json!(75.5);
    other["propertyAddress"]["address1"] = json!("200 Sample St");
    fixtures.accounts.as_array_mut().unwrap().push(other);
}

#[test]
fn a_login_with_two_accounts_answers_for_the_one_named() {
    let mut fixtures = Fixtures::new();
    second_account(&mut fixtures);
    let tools = tools_over(fixtures);

    let got = ok(&tools, "campbell_balance", json!({}));
    assert_eq!(got["account"], "100 Sample St");
    assert_eq!(got["accounts"].as_array().unwrap().len(), 2);

    let got = ok(&tools, "campbell_balance", json!({"account": "200 Sample"}));
    assert_eq!(got["balance"], "75.50");

    let text = error(&tools, "campbell_balance", json!({"account": "300 Sample"}));
    assert!(text.starts_with("not_found:"), "{text}");
}

#[test]
fn bills_are_posted_charges_plus_what_is_assessed_newest_first() {
    let got = ok(&tools(), "campbell_bills", json!({}));
    assert_eq!(got["returned"], 3);
    let items = got["items"].as_array().unwrap();
    assert_eq!(items[0]["due_date"], "2026-10-01");
    assert_eq!(items[0]["paid"], false);
    assert_eq!(items[0]["amount"], "250.00");
    // The ledger's cents, not dollars: 25000 is 250.00.
    assert_eq!(items[1]["date"], "2026-07-01");
    assert_eq!(items[1]["amount"], "250.00");
    assert_eq!(items[2]["date"], "2026-04-01");
}

#[test]
fn a_charge_the_ledger_already_posted_is_not_a_second_bill() {
    let mut fixtures = Fixtures::new();
    fixtures.charges[0]["dueDate"] = json!("2026-07-01T00:00:00.000Z");
    let got = ok(&tools_over(fixtures), "campbell_bills", json!({}));
    assert_eq!(got["returned"], 2, "{got}");
}

#[test]
fn a_voided_charge_is_not_a_bill() {
    let mut fixtures = Fixtures::new();
    fixtures.ledger[1]["isVoided"] = json!(true);
    let got = ok(&tools_over(fixtures), "campbell_bills", json!({}));
    assert_eq!(got["returned"], 2, "{got}");
}

/// `/Charge` names no account, so with several there is no saying whose it is;
/// the ledger's rows do, and only the named account's come back.
#[test]
fn with_two_accounts_bills_hold_only_rows_that_name_the_account() {
    let mut fixtures = Fixtures::new();
    second_account(&mut fixtures);
    fixtures.ledger[3]["accountNumber"] = json!("SMP200000");
    fixtures.ledger[3]["accountWithSubCode"] = json!("SMP200000");
    let tools = tools_over(fixtures);

    let got = ok(&tools, "campbell_bills", json!({}));
    assert_eq!(got["returned"], 1, "{got}");
    assert_eq!(got["items"][0]["date"], "2026-07-01");

    let got = ok(&tools, "campbell_bills", json!({"account": "200 Sample"}));
    assert_eq!(got["returned"], 1, "{got}");
    assert_eq!(got["items"][0]["date"], "2026-04-01");
}

#[test]
fn a_ledger_row_that_changed_shape_is_an_error_not_no_bills() {
    for field in ["transactionType", "accountWithSubCode"] {
        let mut fixtures = Fixtures::new();
        for row in &mut fixtures.ledger {
            let row = row.as_object_mut().unwrap();
            row.remove(field);
            if field == "accountWithSubCode" {
                row.remove("accountNumber");
            }
        }
        let text = error(&tools_over(fixtures), "campbell_bills", json!({}));
        assert!(text.starts_with("upstream:"), "{field}: {text}");
    }
    let mut fixtures = Fixtures::new();
    fixtures.ledger[1].as_object_mut().unwrap().remove("amount");
    let text = error(&tools_over(fixtures), "campbell_bills", json!({}));
    assert!(text.starts_with("upstream:"), "{text}");
}

#[test]
fn a_withheld_ledger_says_what_the_portal_said() {
    let mut fixtures = Fixtures::new();
    fixtures.ledger.clear();
    fixtures.accounts[0]["blockLedger"] = json!(true);
    fixtures.accounts[0]["blockLedgerMessage"] = json!("Sample block message");
    let text = error(&tools_over(fixtures), "campbell_bills", json!({}));
    assert!(text.starts_with("upstream:"), "{text}");
    assert!(text.contains("Sample block message"), "{text}");

    // Nothing withheld: an empty ledger is an empty ledger.
    let mut fixtures = Fixtures::new();
    fixtures.ledger.clear();
    fixtures.charges = json!([]);
    let got = ok(&tools_over(fixtures), "campbell_bills", json!({}));
    assert_eq!(got["returned"], 0);
}

#[test]
fn payments_filter_and_page() {
    let tools = tools();
    let got = ok(&tools, "campbell_payments", json!({}));
    assert_eq!(got["returned"], 5);
    assert_eq!(got["items"][0]["date"], "2026-07-04");
    assert_eq!(got["items"][0]["amount"], "250.00");
    assert_eq!(got["items"][0]["status"], "posted");
    assert!(got["items"][0].get("method").is_none());

    let got = ok(
        &tools,
        "campbell_payments",
        json!({"since": "2026-01-01", "response_format": "detailed"}),
    );
    assert_eq!(got["returned"], 3);
    assert_eq!(got["items"][0]["method"], "ACH Draft");

    let first = ok(&tools, "campbell_payments", json!({"limit": 2}));
    assert_eq!(first["returned"], 2);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = ok(
        &tools,
        "campbell_payments",
        json!({"limit": 2, "cursor": cursor}),
    );
    assert_eq!(second["items"][0]["date"], "2026-01-04");
    let text = error(
        &tools,
        "campbell_payments",
        json!({"limit": 2, "cursor": cursor, "since": "2026-01-01"}),
    );
    assert!(text.starts_with("usage:"), "{text}");
}

#[test]
fn a_pending_payment_says_so() {
    let mut fixtures = Fixtures::new();
    fixtures.payments[0]["pending"] = json!(true);
    let got = ok(&tools_over(fixtures), "campbell_payments", json!({}));
    assert_eq!(got["items"][0]["status"], "pending");
}

#[test]
fn a_payment_row_that_changed_shape_is_an_error() {
    let mut fixtures = Fixtures::new();
    fixtures.payments[0]
        .as_object_mut()
        .unwrap()
        .remove("amount");
    let text = error(&tools_over(fixtures), "campbell_payments", json!({}));
    assert!(text.starts_with("upstream:"), "{text}");
}

#[test]
fn requests_are_open_ones_unless_asked() {
    let tools = tools();
    let got = ok(&tools, "campbell_requests", json!({}));
    assert_eq!(got["returned"], 0, "the fixture's three are closed");

    let got = ok(&tools, "campbell_requests", json!({"include_closed": true}));
    assert_eq!(got["returned"], 3);
    let first = &got["items"][0];
    assert_eq!(first["submitted"], "2024-10-13");
    assert_eq!(first["kind"], "Work Order");
    assert_eq!(first["status"], "Closed");
    assert!(first.get("id").is_none());
    // An empty subject is left out rather than sent as "".
    assert!(got["items"][1].get("subject").is_none());

    let got = ok(
        &tools,
        "campbell_requests",
        json!({"include_closed": true, "since": "2024-05-01", "until": "2024-06-01",
               "response_format": "detailed"}),
    );
    assert_eq!(got["returned"], 1);
    assert_eq!(got["items"][0]["id"], 1040);
}

#[test]
fn a_request_row_that_changed_shape_is_an_error() {
    let mut fixtures = Fixtures::new();
    fixtures.requests[0]
        .as_object_mut()
        .unwrap()
        .remove("closed");
    let text = error(&tools_over(fixtures), "campbell_requests", json!({}));
    assert!(text.starts_with("upstream:"), "{text}");
}

#[test]
fn a_document_row_never_carries_its_link() {
    let tools = tools();
    for args in [
        json!({}),
        json!({"response_format": "detailed"}),
        json!({"query": "statement", "limit": 200, "response_format": "detailed"}),
    ] {
        let result = call(&tools, "campbell_documents", args.clone());
        assert_fits(&tools, "campbell_documents", &result);
        assert!(!result.text().contains("example.invalid"), "{args}");
        assert!(!result.text().contains("url"), "{args}");
    }
}

#[test]
fn documents_list_a_folder_or_search_the_library() {
    let tools = tools();
    let got = ok(&tools, "campbell_documents", json!({}));
    assert_eq!(got["association"], "Sample Association, Inc");
    assert_eq!(got["items"][0]["kind"], "folder");
    assert_eq!(got["items"][0]["folder_id"], 1050);
    assert!(got["items"][0].get("id").is_none());

    let got = ok(&tools, "campbell_documents", json!({"query": "statement"}));
    assert_eq!(got["items"][0]["kind"], "file");
    assert!(got["items"][0].get("folder_id").is_none());
    assert!(got["items"][0]["path"].as_str().unwrap().contains("Audit"));

    let text = error(
        &tools,
        "campbell_documents",
        json!({"query": "statement", "folder": 1050}),
    );
    assert!(text.starts_with("usage:"), "{text}");
}

#[test]
fn a_search_walks_the_pages_it_needs_and_says_when_more_remain() {
    let fixtures = Fixtures::new();
    let reads = fixtures.reads.clone();
    let tools = tools_over(fixtures);
    // One page holds 15 of the fixture's three pages, so 10 needs one search.
    let got = ok(
        &tools,
        "campbell_documents",
        json!({"query": "statement", "limit": 10}),
    );
    assert_eq!(got["returned"], 10);
    assert!(got["next_cursor"].is_string(), "{got}");
    assert_eq!(reads.get(), 2, "the account list and one search page");

    reads.set(0);
    // 20 needs a second page.
    let got = ok(
        &tools,
        "campbell_documents",
        json!({"query": "statement", "limit": 20}),
    );
    assert_eq!(got["returned"], 20);
    assert_eq!(reads.get(), 3, "the account list and two search pages");
}

#[test]
fn a_document_answer_that_changed_shape_is_an_error_not_an_empty_library() {
    let mut fixtures = Fixtures::new();
    fixtures.library = json!({"member": []});
    let text = error(&tools_over(fixtures), "campbell_documents", json!({}));
    assert!(text.starts_with("upstream:"), "{text}");

    let mut fixtures = Fixtures::new();
    fixtures.search = json!({"results": []});
    let text = error(
        &tools_over(fixtures),
        "campbell_documents",
        json!({"query": "x"}),
    );
    assert!(text.starts_with("upstream:"), "{text}");
}

#[test]
fn the_configured_association_wins_and_its_name_is_only_stated_when_known() {
    let mut fixtures = Fixtures::new();
    fixtures.configured = Some(999);
    let got = ok(&tools_over(fixtures), "campbell_documents", json!({}));
    assert!(got.get("association").is_none(), "{got}");
}

/// A bad argument is answered before any read, so it never touches a credential.
#[test]
fn bad_arguments_fail_before_any_read() {
    let fixtures = Fixtures::new();
    let reads = fixtures.reads.clone();
    let tools = tools_over(fixtures);
    for (name, args) in [
        ("campbell_bills", json!({"since": "yesterday"})),
        ("campbell_payments", json!({"limit": 0})),
        ("campbell_requests", json!({"since": "2026-13-01"})),
        (
            "campbell_requests",
            json!({"since": "2026-02-01", "until": "2026-01-01"}),
        ),
        ("campbell_documents", json!({"cursor": "not-one-of-ours"})),
        ("campbell_documents", json!({"query": "a", "folder": 1})),
        ("campbell_balance", json!({"acount": "typo"})),
    ] {
        let text = error(&tools, name, args);
        assert!(text.starts_with("usage:"), "{name}: {text}");
    }
    assert_eq!(reads.get(), 0);
}

#[test]
fn a_lapsed_session_asks_for_a_sign_in_in_a_terminal() {
    let mut fixtures = Fixtures::new();
    fixtures.failure =
        Some(|| CliError::Auth("the portal rejected the session — run `cpmfl auth login`".into()));
    let text = error(&tools_over(fixtures), "campbell_balance", json!({}));
    assert!(text.starts_with("auth:"), "{text}");
    assert_eq!(
        text.matches("cpmfl auth login").count(),
        1,
        "one command, once: {text}"
    );
}

#[test]
fn a_token_from_the_variable_names_the_variable_as_the_remedy() {
    let mut fixtures = Fixtures::new();
    fixtures.failure = Some(|| CliError::Auth("the portal rejected the session".into()));
    let tools = with_reads(fixtures, true);
    let text = error(&tools, "campbell_balance", json!({}));
    assert!(text.contains("$CPMFL_TOKEN"), "{text}");
    assert!(!text.contains("cpmfl auth login"), "{text}");
    assert!(pk_cli_mcp::instructions(&tools).contains("$CPMFL_TOKEN"));
}

#[test]
fn a_rejected_request_is_the_portal_failing_not_a_bad_argument() {
    let e = portal_error(CliError::Usage("nope".into()));
    assert_eq!(e.exit_code(), 5);
    assert_eq!(portal_error(CliError::Auth("x".into())).exit_code(), 3);
}

struct Keychain(Option<&'static str>);

impl KeychainRead for Keychain {
    fn service(&self) -> &str {
        "piekstra.cpmfl-test"
    }
    fn get(&self, _account: &str) -> Result<Option<Secret>, CliError> {
        Ok(self.0.map(Secret::new))
    }
}

#[test]
fn the_token_comes_from_the_variable_then_the_keychain() {
    let unset = "CPMFL_TEST_TOKEN_UNSET";
    let got = resolve_token(&Keychain(Some("from-keychain")), unset).unwrap();
    assert_eq!(got.expose(), "from-keychain");

    let e = resolve_token(&Keychain(None), unset).err().unwrap();
    assert_eq!(e.exit_code(), 3, "{e}");

    // `CARGO_MANIFEST_DIR` is set for every test run, and stands in for a token.
    let got = resolve_token(&Keychain(Some("from-keychain")), "CARGO_MANIFEST_DIR").unwrap();
    assert_eq!(got.expose(), env!("CARGO_MANIFEST_DIR"));
}

#[test]
fn a_token_past_its_own_expiry_is_refused_without_a_request() {
    // `{"alg":"none"}.{"exp":1}.` in base64url.
    let token = "eyJhbGciOiJub25lIn0.eyJleHAiOjF9.";
    let e = Portal::with_live_token(&Config::default(), Secret::new(token))
        .err()
        .expect("expired");
    assert_eq!(e.exit_code(), 3, "{e}");
}
