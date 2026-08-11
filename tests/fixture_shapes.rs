//! Contract tests against captured (scrubbed) portal responses.
//!
//! These guard the assumption every command makes: that the portal's JSON
//! still carries the fields the DTOs read. If Vantaca renames `ledgerDate` or
//! moves `autoDraftSettings`, this fails loudly instead of quietly rendering
//! empty columns.
//!
//! Fully offline — no network, no credentials.

use serde_json::Value;

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parsing {path}: {e}"))
}

/// Every fixture that a command reads, so a missing file fails here rather
/// than in whichever test happens to run first.
const ALL_FIXTURES: &[&str] = &[
    "account.json",
    "ledger.json",
    "payments.json",
    "charges.json",
    "autodraft.json",
    "recurring_payment.json",
    "property_owners.json",
    "homeowner_requests.json",
    "request_types.json",
    "directories.json",
    "board_directory.json",
    "document_search.json",
    "pay_features.json",
    "user_configuration.json",
    "homeowner_preferences.json",
];

#[test]
fn every_fixture_parses() {
    for name in ALL_FIXTURES {
        let _ = fixture(name);
    }
}

#[test]
fn account_shape() {
    let v = fixture("account.json");
    let a = &v.as_array().expect("array of accounts")[0];
    // `summary` / `balance` read these four.
    assert!(a["balance"].is_number());
    assert!(a["accountNumber"].is_string());
    assert!(a["nextChargeDate"].is_string());
    assert!(a["autoDraftEnrolled"].is_boolean());
    // `account` adds the association, owner, and address blocks.
    assert!(a["associationId"].is_number());
    assert!(a["associationName"].is_string());
    assert!(a["managementCompany"].is_string());
    assert!(a["ownerName"].is_string());
    assert!(a["isPrimaryOwner"].is_boolean());
    assert!(a["propertyAddress"]["address1"].is_string());
    assert!(a["propertyAddress"]["postalCode"].is_string());
    // Portal-side blocks: present even when false.
    assert!(a["blockLedger"].is_boolean());
    assert!(a["blockPayments"].is_boolean());
    assert!(a.get("blockLedgerMessage").is_some());
}

#[test]
fn paged_envelope_is_consistent() {
    // Everything except document search uses member/totalItems.
    for name in ["ledger.json", "payments.json", "homeowner_requests.json"] {
        let v = fixture(name);
        assert!(v["page"].is_number(), "{name} page");
        assert!(v["lastPage"].is_number(), "{name} lastPage");
        assert!(v["totalItems"].is_number(), "{name} totalItems");
        assert!(v["member"].is_array(), "{name} member");
    }
}

#[test]
fn ledger_shape() {
    let v = fixture("ledger.json");
    let rows = v["member"].as_array().expect("ledger rows");
    assert!(!rows.is_empty());
    for t in rows {
        assert!(t["ledgerDate"].is_string());
        assert!(t["amount"].is_number());
        assert!(t["runningBalance"].is_number());
        assert!(t["description"].is_string());
        assert!(t["transactionType"].is_string());
        assert!(t["isVoided"].is_boolean());
    }
}

/// The trap this CLI exists to not fall into: the same $250.00 is `25000` on
/// the ledger and `250` on charges/payments. If a future capture makes these
/// agree numerically, the scale assumption in `commands::money_cents` needs
/// revisiting — not this test silently updating.
#[test]
fn ledger_reports_cents_while_charges_report_dollars() {
    let ledger = fixture("ledger.json");
    let charges = fixture("charges.json");
    let payments = fixture("payments.json");

    let ledger_amount = ledger["member"][0]["amount"]
        .as_f64()
        .expect("ledger amount")
        .abs();
    let charge_amount = charges[0]["amount"].as_f64().expect("charge amount").abs();
    let payment_amount = payments["member"][0]["amount"]
        .as_f64()
        .expect("payment amount")
        .abs();

    assert_eq!(charge_amount, payment_amount, "dollar endpoints agree");
    assert_eq!(
        ledger_amount,
        charge_amount * 100.0,
        "ledger is cents where charges are dollars"
    );
}

#[test]
fn payments_shape() {
    let v = fixture("payments.json");
    for p in v["member"].as_array().expect("payments") {
        assert!(p["id"].is_string());
        assert!(p["date"].is_string());
        assert!(p["amount"].is_number());
        assert!(p["description"].is_string());
        assert!(p["paymentType"].is_string());
        assert!(p["pending"].is_boolean());
    }
}

#[test]
fn charges_shape() {
    let v = fixture("charges.json");
    for c in v.as_array().expect("charges") {
        assert!(c["id"].is_number());
        assert!(c["amount"].is_number());
        assert!(c["dueDate"].is_string());
        assert!(c["description"].is_string());
    }
}

#[test]
fn autodraft_shape() {
    let v = fixture("autodraft.json");
    let a = &v.as_array().expect("autodraft records")[0];
    assert!(a["enrolled"].is_boolean());
    assert!(a["nextPullDate"].is_string());
    assert!(a["bankAccountLast4"].is_string());
    let s = &a["autoDraftSettings"];
    assert!(s["autoDraftDay"].is_number());
    assert!(s["generateDaysInAdvanced"].is_number());
    assert!(s["autoDraftAmount"].is_string());
    assert!(s["autoDraftIncludeCharges"].is_string());
}

/// A recurring autopay rule of "draft whatever is charged" has a **null**
/// amount. `commands::money_opt_cents` depends on that staying null rather
/// than becoming a misleading zero.
#[test]
fn recurring_payment_amount_may_be_null() {
    let v = fixture("recurring_payment.json");
    let r = &v.as_array().expect("recurring records")[0];
    assert!(r.get("amount").is_some(), "the key must exist");
    assert!(r["amount"].is_null());
    assert!(r["interval"].is_string());
    assert!(r["paymentMethod"]["bankAccount"]["last4"].is_string());
}

/// `scheduled` reports `nextPaymentDate`, not `anchorDate`.
///
/// They are different things and the fixture proves it: the anchor is when
/// enrollment started (in the past), the next payment is the upcoming draft.
/// Reading the anchor and labelling it "next" told the user their next payment
/// had already happened.
#[test]
fn recurring_payment_distinguishes_anchor_from_next_payment() {
    let v = fixture("recurring_payment.json");
    let r = &v.as_array().expect("recurring records")[0];
    let anchor = r["anchorDate"].as_str().expect("anchorDate");
    let next = r["nextPaymentDate"].as_str().expect("nextPaymentDate");
    assert!(
        anchor < next,
        "fixture must keep an anchor ({anchor}) earlier than the next payment ({next}) \
         so the two can't be confused"
    );
    // The fields `scheduled` reads to decide how to present an amount.
    assert!(r["payFullBalance"].is_boolean());
    assert!(r.get("cancelled").is_some());
}

/// Scheduled payments and the autopay fee are **cents**, unlike `/Charge` and
/// `/Payment`. The portal's own UI divides both by 100 before display; a raw
/// `applicationFee` reads as a $250 charge for a $2.50 fee.
#[test]
fn scheduled_amounts_and_the_autopay_fee_are_cents() {
    let autodraft = fixture("autodraft.json");
    let fee = autodraft[0]["autoDraftSettings"]["applicationFee"]
        .as_f64()
        .expect("applicationFee");
    assert_eq!(fee, 250.0, "the dummy fee is 250 cents = $2.50");
    assert!(
        fee.fract() == 0.0,
        "a cents amount is a whole number; a fractional value would mean the \
         endpoint switched to dollars"
    );
}

#[test]
fn property_owners_shape() {
    let v = fixture("property_owners.json");
    let p = &v["member"].as_array().expect("properties")[0];
    assert!(p["accountNumber"].is_string());
    assert!(p["association"]["id"].is_number());
    assert!(p["association"]["name"].is_string());
    assert!(p["address"]["address1"].is_string());
    assert!(p["address"]["city"].is_string());
}

#[test]
fn homeowner_requests_shape() {
    let v = fixture("homeowner_requests.json");
    for r in v["member"].as_array().expect("requests") {
        assert!(r["id"].is_number());
        assert!(r["submitted"].is_string());
        assert!(r["type"].is_string());
        assert!(r["status"].is_string());
        assert!(r["closed"].is_boolean());
        // `subject` is present but empty for ARC requests — key must exist.
        assert!(r.get("subject").is_some());
    }
}

#[test]
fn request_types_shape() {
    let v = fixture("request_types.json");
    for t in v["requestTypes"].as_array().expect("request types") {
        assert!(t["actionTypeId"].is_string());
        assert!(t["description"].is_string());
        assert!(t["categoryName"].is_string());
        assert!(t["requiresAssociation"].is_boolean());
    }
}

#[test]
fn directories_shape() {
    let v = fixture("directories.json");
    let item = &v["items"].as_array().expect("directory items")[0];
    assert!(item["id"].is_number());
    assert!(item["type"].is_string());
    assert!(item["name"].is_string());
    assert!(item["lastModified"].is_string());
    assert!(item["visibility"].is_string());
    assert!(v["breadcrumbs"].is_array());
}

/// Document search answers with its own envelope, not the `member` one.
#[test]
fn document_search_uses_its_own_envelope() {
    let v = fixture("document_search.json");
    assert!(v["totalResults"].is_number());
    assert!(v["totalPages"].is_number());
    assert!(v["pageSize"].is_number());
    assert!(v["documents"].is_array());
    assert!(
        v.get("member").is_none(),
        "search must not grow a member key"
    );
    assert!(v.get("totalItems").is_none());

    let d = &v["documents"].as_array().expect("documents")[0];
    assert!(d["name"].is_string());
    assert!(d["fullPath"].is_string());
    assert!(d["documentUrl"].is_string());
}

#[test]
fn board_directory_shape() {
    let v = fixture("board_directory.json");
    for d in v["member"].as_array().expect("directory") {
        assert!(d["directoryId"].is_number());
        assert!(d["displayName"].is_string());
        assert!(d["roleName"].is_string());
        // Phone is often empty; the key must still be there.
        assert!(d.get("phone").is_some());
        assert!(d.get("email").is_some());
    }
}

#[test]
fn features_shape() {
    let v = fixture("pay_features.json");
    for key in ["vantacaPay", "expressPay", "guestPay", "smsIntegration"] {
        assert!(v[key].is_boolean(), "{key} should be a boolean");
    }
    assert!(v["domain"].is_string());
}

#[test]
fn user_configuration_shape() {
    let v = fixture("user_configuration.json");
    assert!(v["billingTextNotificationsAvailable"].is_boolean());
    assert!(v["availableGeneralCommunicationPreferences"].is_array());
}

#[test]
fn homeowner_preferences_shape() {
    let v = fixture("homeowner_preferences.json");
    // What `profile` reads: communication channels and directory privacy.
    assert!(v["billingCommunicationPreference"].is_string());
    assert!(v["generalCommunicationPreference"].is_string());
    assert!(v["receivePayBillingText"].is_boolean());
    assert!(v["sendPayConfirmationEmail"].is_boolean());
    for key in [
        "hideNameInDirectory",
        "hideEmailInDirectory",
        "hidePhoneInDirectory",
        "hidePropertyInDirectory",
    ] {
        assert!(v[key].is_boolean(), "{key} should be a boolean");
    }
    assert!(v["email"].is_string());
    assert!(v["mailingAddressId"].is_number());
}

/// Mechanical enforcement of `tests/fixtures/README.md`.
///
/// Deliberately an **allow**-list of dummy values rather than a deny-list of
/// real ones. A deny-list would have to spell out the account holder's name,
/// address, and account number in a tracked file — reintroducing exactly what
/// the policy exists to keep out — and would only ever catch the identifiers
/// someone remembered to add. Pinning each identity-bearing field to its known
/// dummy catches anything unscrubbed, including kinds of data nobody
/// anticipated.
///
/// When a new identity-bearing key appears in a capture, add it here with its
/// dummy value; the `unchecked identity-shaped key` assertion below will point
/// at anything missed.
#[test]
fn fixtures_carry_no_real_identifiers() {
    // key -> the only values a scrubbed fixture may carry.
    const EXPECTED: &[(&str, &[&str])] = &[
        ("ownerName", &["Sample Owner"]),
        ("displayName", &["Sample Owner"]),
        ("firstName", &["Sample Owner"]),
        ("lastName", &["Sample Owner"]),
        ("billingName", &["Sample Owner"]),
        ("eMail", &["owner@example.invalid", ""]),
        ("email", &["owner@example.invalid", ""]),
        ("associationName", &["Sample Association, Inc"]),
        ("associationCode", &["SAMPLE"]),
        ("code", &["SAMPLE"]),
        ("managementCompany", &["Sample Property Management"]),
        ("address1", &["100 Sample St"]),
        ("address", &["100 Sample St, Sample City, ST 00000"]),
        ("city", &["Sample City"]),
        ("stateProvince", &["ST"]),
        ("postalCode", &["00000"]),
        ("phone", &["555-0100", ""]),
        ("phoneNumber", &["555-0100", ""]),
        ("accountNumber", &["SMP100000"]),
        ("account", &["SMP100000"]),
        ("accountWithSubCode", &["SMP100000"]),
        ("last4", &["0000"]),
        ("bankAccountLast4", &["0000"]),
        ("bankName", &["Sample Bank"]),
        (
            "documentUrl",
            &["https://example.invalid/document-redacted", ""],
        ),
        ("domain", &["https://portal.example.invalid"]),
    ];

    /// Keys that look like they carry identity but have no rule yet — a
    /// backstop so a new capture can't smuggle one in unnoticed.
    const IDENTITY_HINTS: &[&str] = &[
        "name", "email", "mail", "phone", "address", "account", "url", "ssn", "owner", "last4",
    ];
    /// …minus the ones that are structural rather than personal.
    const NOT_IDENTITY: &[&str] = &[
        "associationName",
        "bankName",
        "displayName",
        "firstName",
        "lastName",
        "billingName",
        "ownerName",
        "roleName",
        "categoryName",
        "name",
        "fullPath",
        "accountStatus",
        "accountSetupModal",
        "blockLedgerMessage",
        // A relationship-type enum ("homeownerAddress"), not a mailing address.
        "mailRelTypeId",
    ];

    fn expected_for(key: &str) -> Option<&'static [&'static str]> {
        EXPECTED.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    }

    /// Walk every string in the document, checking values by key and by shape.
    fn check(path: &str, key: Option<&str>, v: &Value) {
        match v {
            Value::Object(map) => {
                for (k, x) in map {
                    check(path, Some(k), x);
                }
            }
            Value::Array(items) => {
                for x in items {
                    check(path, key, x);
                }
            }
            Value::String(s) => {
                if let Some(key) = key {
                    if let Some(allowed) = expected_for(key) {
                        assert!(
                            allowed.contains(&s.as_str()),
                            "{path}: `{key}` = {s:?} is not a scrubbed dummy \
                             (expected one of {allowed:?})"
                        );
                    } else {
                        let lower = key.to_lowercase();
                        let identity_shaped = IDENTITY_HINTS.iter().any(|h| lower.contains(h));
                        assert!(
                            !identity_shaped || NOT_IDENTITY.contains(&key),
                            "{path}: `{key}` looks identity-bearing but has no scrubbing \
                             rule — add it to EXPECTED (or NOT_IDENTITY if it is structural)"
                        );
                    }
                }
                // Shape rules, applied to every string regardless of key: these
                // catch identifiers hiding in free text such as document names.
                if s.contains('@') && s.contains('.') {
                    assert!(
                        s.ends_with("@example.invalid"),
                        "{path}: {s:?} looks like a real email address"
                    );
                }
                if s.starts_with("http://") || s.starts_with("https://") {
                    let host = s
                        .split("//")
                        .nth(1)
                        .and_then(|rest| rest.split('/').next())
                        .unwrap_or_default();
                    assert!(
                        host.ends_with(".invalid"),
                        "{path}: {s:?} points at a real host — URLs must be .invalid"
                    );
                }
                // A JWT would start with the base64 of `{"alg"`.
                assert!(
                    !s.starts_with("eyJ"),
                    "{path}: {s:?} looks like a captured token"
                );
            }
            _ => {}
        }
    }

    let dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("fixtures dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let v: Value = serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()));
        check(&name, None, &v);
        checked += 1;
    }
    assert_eq!(
        checked,
        ALL_FIXTURES.len(),
        "every fixture must be scanned; update ALL_FIXTURES when adding one"
    );
}
