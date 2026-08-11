//! Offline black-box tests: the command surface, the error contract, and the
//! exit codes. Nothing here touches the network or the keychain.

use assert_cmd::Command;
use predicates::prelude::*;

fn cpmfl() -> Command {
    Command::cargo_bin("cpmfl").expect("binary builds")
}

/// Every top-level command, for the help-tree walk below.
const COMMANDS: &[&str] = &[
    "auth",
    "config",
    "summary",
    "balance",
    "account",
    "properties",
    "transactions",
    "payments",
    "charges",
    "autopay",
    "scheduled",
    "payment-methods",
    "documents",
    "requests",
    "directory",
    "calendar",
    "reservations",
    "profile",
    "association",
    "api",
    "self-update",
    "completions",
    "info",
];

#[test]
fn top_level_help_lists_the_surface() {
    let out = cpmfl().arg("--help").assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    for cmd in COMMANDS {
        assert!(stdout.contains(cmd), "`{cmd}` missing from --help");
    }
}

/// Rendering a subcommand's help forces clap's debug assertions to run over
/// that subtree, which is what catches conflicting short flags (e.g. an
/// `api -q` colliding with the global `--quiet`).
#[test]
fn every_subcommand_help_renders() {
    for cmd in COMMANDS {
        cpmfl()
            .args([cmd, "--help"])
            .assert()
            .success()
            .stdout(predicate::str::is_empty().not());
    }
}

#[test]
fn nested_subcommand_help_renders() {
    for (group, sub) in [
        ("auth", "login"),
        ("auth", "status"),
        ("auth", "logout"),
        ("auth", "set-credential"),
        ("config", "set"),
        ("config", "show"),
        ("documents", "list"),
        ("documents", "search"),
        ("documents", "download"),
        ("requests", "list"),
        ("requests", "types"),
    ] {
        cpmfl().args([group, sub, "--help"]).assert().success();
    }
}

#[test]
fn version_prints_the_crate_version() {
    cpmfl()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

/// `info` is the family's capability-discovery DTO and must be emitted without
/// credentials or network.
#[test]
fn info_emits_cli_info_dto() {
    let out = cpmfl().arg("info").assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("info emits JSON");

    assert_eq!(v["schema"], "cli-info/v1");
    assert_eq!(v["name"], "cpmfl");
    assert_eq!(v["auth"]["required"], true);
    assert_eq!(v["auth"]["method"], "password");
    // The utility profile is what lets drivers read this CLI with no config.
    assert_eq!(v["profiles"][0], "utility/v1");

    let capabilities = v["capabilities"].as_array().expect("capabilities array");
    for expected in ["summary", "balance", "transactions", "documents"] {
        assert!(
            capabilities.iter().any(|c| c == expected),
            "`{expected}` missing from info capabilities"
        );
    }
}

#[test]
fn completions_render_for_every_shell() {
    for shell in ["bash", "zsh", "fish"] {
        cpmfl()
            .args(["completions", shell])
            .assert()
            .success()
            .stdout(predicate::str::is_empty().not());
    }
}

/// Bad arguments must be rejected as usage errors (exit 2) *before* anything
/// reaches the keychain or the network — so `--help` and typos never prompt,
/// hang, or hit the portal.
#[test]
fn malformed_dates_are_usage_errors() {
    for args in [
        vec!["transactions", "--since", "08/01/2026"],
        vec!["transactions", "--until", "not-a-date"],
        vec!["payments", "--since", "2026-13-01"],
        vec!["charges", "--until", "2026-01-32"],
        vec!["calendar", "--since", "01-01-2026"],
        vec!["requests", "list", "--since", "nope"],
    ] {
        cpmfl().args(&args).assert().code(2);
    }
}

#[test]
fn inverted_ranges_are_usage_errors() {
    cpmfl()
        .args([
            "transactions",
            "--since",
            "2026-06-01",
            "--until",
            "2026-01-01",
        ])
        .assert()
        .code(2);
    cpmfl()
        .args(["calendar", "--since", "2026-06-01", "--until", "2026-01-01"])
        .assert()
        .code(2);
}

#[test]
fn unknown_config_keys_are_usage_errors() {
    cpmfl()
        .args(["config", "set", "not_a_key", "value"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unknown config key"));
    cpmfl()
        .args(["config", "unset", "not_a_key"])
        .assert()
        .code(2);
}

/// `association_id` is the one numeric config key; a non-number is a usage
/// error rather than a silently ignored write.
#[test]
fn non_numeric_association_id_is_a_usage_error() {
    cpmfl()
        .args(["config", "set", "association_id", "abc"])
        .assert()
        .code(2);
}

#[test]
fn empty_document_search_term_is_a_usage_error() {
    cpmfl()
        .args(["documents", "search", "   "])
        .assert()
        .code(2);
}

/// `documents download` must reject bad arguments before anything reaches the
/// keychain or the network: a non-numeric id and a missing `--output` are both
/// clap-level usage errors.
#[test]
fn malformed_document_download_args_are_usage_errors() {
    cpmfl()
        .args(["documents", "download", "not-a-number", "--output", "x.pdf"])
        .assert()
        .code(2);
    cpmfl()
        .args(["documents", "download", "123"])
        .assert()
        .code(2);
}

#[test]
fn malformed_api_query_is_a_usage_error() {
    cpmfl()
        .args(["api", "/Ledger", "--query", "no-equals-sign"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("KEY=VALUE"));
}

/// The `api` passthrough is GET-only by construction: there is no method
/// argument to widen it. If this test starts failing because a method flag was
/// added, the read-only guarantee in AGENTS.md needs revisiting first.
#[test]
fn api_passthrough_exposes_no_write_method() {
    let out = cpmfl().args(["api", "--help"]).assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    for flag in ["--method", "--data", "POST", "DELETE"] {
        assert!(
            !stdout.contains(flag),
            "`api --help` mentions {flag:?} — the CLI is supposed to be read-only"
        );
    }
}

/// The surface must carry no mutating verbs. A new command named after one of
/// these means the read-only claim is stale.
///
/// Checks command *names* rather than help text: a read command may perfectly
/// well describe something mutable (`autopay` reports on "enrollment"), and a
/// substring scan over descriptions flags that as a violation.
#[test]
fn the_surface_carries_no_mutating_commands() {
    const MUTATING: &[&str] = &[
        "pay",
        "enroll",
        "submit",
        "reserve",
        "book",
        "cancel",
        "delete",
        "remove",
        "approve",
        "decline",
        "vote",
        "upload",
        "create",
        "update",
        "edit",
        "set-autopay",
    ];

    let out = cpmfl().arg("info").assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("info emits JSON");

    let mut names: Vec<String> = v["capabilities"]
        .as_array()
        .expect("capabilities")
        .iter()
        .map(|c| c.as_str().unwrap_or_default().to_string())
        .collect();

    // Subcommands too — a write would most naturally be added under a group.
    for group in ["documents", "requests", "auth", "config"] {
        let help = cpmfl().args([group, "--help"]).assert().success();
        let text = String::from_utf8_lossy(&help.get_output().stdout).to_string();
        names.extend(
            text.lines()
                .skip_while(|l| !l.starts_with("Commands:"))
                .skip(1)
                .take_while(|l| l.starts_with("  ") || l.trim().is_empty())
                .filter_map(|l| l.split_whitespace().next())
                .map(str::to_string),
        );
    }

    for name in &names {
        let lower = name.to_lowercase();
        for banned in MUTATING {
            assert!(
                !lower.split(['-', '_']).any(|part| part == *banned),
                "command {name:?} looks like a write — the CLI is read-only"
            );
        }
    }
}
