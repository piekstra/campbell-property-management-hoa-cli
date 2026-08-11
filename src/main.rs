//! `cpmfl` — piekstra-family CLI for the Campbell Property Management
//! (Vantaca) HOA homeowner portal.
//!
//! Conforms to piekstra-cli/1 and the `utility/v1` domain profile. Read-only
//! today: every command observes, none mutate. Payments, autopay enrollment,
//! request submission, and profile edits are deliberately out of scope —
//! `docs/api.md` catalogs the endpoints they would use.

mod client;
mod commands;
mod config;
mod dates;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use pk_cli_auth::token as jwt;
use pk_cli_auth::{AuthStatus, LoginArgs, LogoutArgs, SetCredentialArgs};
use pk_cli_config::ConfigStore;
use pk_cli_core::info::{AuthInfo, CliInfo};
use pk_cli_core::{output, CliError, CommonArgs};
use pk_cli_secrets::CredentialStore;
use pk_cli_selfupdate::{SelfUpdateArgs, Updater};
use pk_cli_utility::RangeArgs;

use client::Portal;
use commands::{account, api, community, documents, money, profile, requests, Ctx};
use config::{Config, KEYCHAIN_ACCOUNT, TOKEN_ACCOUNT};

const BIN: &str = "cpmfl";
const REPO: &str = "piekstra/campbell-property-management-hoa-cli";

/// Campbell Property Management HOA portal from the command line — balance,
/// ledger, payments, documents, and requests. Unofficial.
#[derive(Parser, Debug)]
#[command(name = BIN, version, about, long_about = None)]
struct Cli {
    #[command(flatten)]
    common: CommonArgs,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Portal login, session status, and credential management.
    #[command(subcommand)]
    Auth(AuthCmd),
    /// Non-secret settings.
    #[command(subcommand)]
    Config(ConfigCmd),
    /// Account card: balance, next charge, autopay (utility-summary/v1).
    Summary,
    /// Current account balance (utility-summary/v1).
    Balance,
    /// Full account detail: association, owner, property, portal blocks.
    Account,
    /// Properties and ledgers this login can see.
    Properties,
    /// Account ledger: charges, payments, credits, running balance.
    Transactions(RangeArgs),
    /// Posted and pending payments.
    Payments(RangeArgs),
    /// Assessed charges and their due dates.
    Charges(RangeArgs),
    /// Autopay (AutoDraft) enrollment and schedule.
    Autopay,
    /// Recurring and one-off payments already scheduled.
    Scheduled,
    /// Saved payment methods (last four digits only).
    PaymentMethods,
    /// Association document library.
    #[command(subcommand)]
    Documents(documents::Cmd),
    /// Homeowner service requests.
    #[command(subcommand)]
    Requests(requests::Cmd),
    /// Board members and management contacts.
    Directory {
        /// Maximum entries to return.
        #[arg(long, value_name = "N")]
        limit: Option<u32>,
    },
    /// Community calendar events.
    Calendar {
        /// Only events on or after this date (ISO `YYYY-MM-DD`).
        #[arg(long, value_name = "YYYY-MM-DD")]
        since: Option<String>,
        /// Only events on or before this date (ISO `YYYY-MM-DD`).
        #[arg(long, value_name = "YYYY-MM-DD")]
        until: Option<String>,
        /// Maximum events to return.
        #[arg(long, value_name = "N")]
        limit: Option<u32>,
    },
    /// Amenity reservations you hold.
    Reservations,
    /// Your communication and directory-privacy settings.
    Profile,
    /// Association configuration and enabled portal features.
    Association,
    /// Raw portal API passthrough (GET only).
    Api(api::ApiArgs),
    /// Update to the latest release from GitHub.
    SelfUpdate(SelfUpdateArgs),
    /// Print a shell completion script.
    Completions { shell: Shell },
    /// Machine-readable capability discovery (cli-info/v1).
    Info,
}

#[derive(Subcommand, Debug)]
enum AuthCmd {
    /// Log in to the portal and cache the resulting bearer token.
    Login(LoginArgs),
    /// Report credential and session state (auth-status/v1).
    Status,
    /// Clear the cached token; --forget also removes the stored password.
    Logout(LogoutArgs),
    /// Raw keychain write for rotation / headless setup.
    SetCredential(SetCredentialArgs),
}

#[derive(Subcommand, Debug)]
enum ConfigCmd {
    /// Print the resolved config file path.
    Path,
    /// Show the effective configuration.
    Show,
    /// Set a config key.
    Set { key: String, value: String },
    /// Remove a config key.
    Unset { key: String },
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(&cli) {
        std::process::exit(output::fail(&e, cli.common.json));
    }
}

fn run(cli: &Cli) -> Result<(), CliError> {
    let store = ConfigStore::new(BIN);
    let creds = CredentialStore::for_binary(BIN);
    let cfg: Config = store.load()?;
    let ctx = Ctx {
        common: &cli.common,
        cfg: &cfg,
        creds: &creds,
    };

    match &cli.command {
        Command::Auth(cmd) => auth(cli, cmd, &store, &creds, &cfg),
        Command::Config(cmd) => config_cmd(cli, cmd, &store),
        Command::Summary | Command::Balance => account::summary(&ctx),
        Command::Account => account::detail(&ctx),
        Command::Properties => account::properties(&ctx),
        Command::Transactions(range) => money::transactions(&ctx, range),
        Command::Payments(range) => money::payments(&ctx, range),
        Command::Charges(range) => money::charges(&ctx, range),
        Command::Autopay => money::autopay(&ctx),
        Command::Scheduled => money::scheduled(&ctx),
        Command::PaymentMethods => money::payment_methods(&ctx),
        Command::Documents(cmd) => documents::run(&ctx, cmd),
        Command::Requests(cmd) => requests::run(&ctx, cmd),
        Command::Directory { limit } => community::directory(&ctx, *limit),
        Command::Calendar {
            since,
            until,
            limit,
        } => community::calendar(&ctx, since.as_deref(), until.as_deref(), *limit),
        Command::Reservations => community::reservations(&ctx),
        Command::Profile => profile::run(&ctx),
        Command::Association => community::association(&ctx),
        Command::Api(args) => api::run(&ctx, args),
        Command::SelfUpdate(args) => Updater {
            repo: REPO.into(),
            binary: BIN.into(),
            target: env!("BUILD_TARGET").into(),
            current: env!("CARGO_PKG_VERSION").into(),
        }
        .run(args, cli.common.json, cli.common.quiet),
        Command::Completions { shell } => {
            clap_complete::generate(*shell, &mut Cli::command(), BIN, &mut std::io::stdout());
            Ok(())
        }
        Command::Info => {
            let info = CliInfo::new(
                BIN,
                env!("CARGO_PKG_VERSION"),
                &format!("https://github.com/{REPO}"),
                AuthInfo {
                    required: true,
                    method: "password".into(),
                    login_hint: Some(format!("{BIN} auth login")),
                },
                &[
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
                ],
            )
            .with_profiles(&[pk_cli_utility::PROFILE]);
            output::json(&serde_json::to_value(&info).unwrap());
            Ok(())
        }
    }
}

fn auth(
    cli: &Cli,
    cmd: &AuthCmd,
    store: &ConfigStore,
    creds: &CredentialStore,
    cfg: &Config,
) -> Result<(), CliError> {
    match cmd {
        AuthCmd::Login(args) => login(cli, args, creds, cfg),
        AuthCmd::Status => {
            let has_password = creds.get(KEYCHAIN_ACCOUNT)?.is_some();
            let cached = creds.get(TOKEN_ACCOUNT)?;
            // The token carries its own expiry, so status can answer honestly
            // without spending a request on the portal.
            let live = cached
                .as_ref()
                .map(|t| match jwt::expiry(t.expose()) {
                    Some(exp) => exp > now_unix(),
                    None => true,
                })
                .unwrap_or(false);

            let mut status = AuthStatus::new(true, live, pk_cli_auth::AuthMethod::Password);
            status.username = cfg.username();
            status.credential_in_keychain = Some(has_password);
            status.authenticated = live;
            // When the session's lifetime is knowable, say so — `auth-status/v1`
            // reserves `expires_at` for exactly this.
            status.expires_at = cached.as_ref().and_then(|t| jwt::expires_at(t.expose()));
            status.emit(cli.common.json);
            Ok(())
        }
        AuthCmd::Logout(args) => {
            creds.delete(TOKEN_ACCOUNT)?;
            if args.forget {
                creds.delete(KEYCHAIN_ACCOUNT)?;
                store.clear()?;
                if !cli.common.quiet {
                    eprintln!("session cleared; password removed");
                }
            } else if !cli.common.quiet {
                eprintln!("session cleared (password kept; use --forget to remove it)");
            }
            Ok(())
        }
        AuthCmd::SetCredential(args) => {
            if creds.get(KEYCHAIN_ACCOUNT)?.is_some() && !args.overwrite {
                return Err(CliError::Usage(
                    "a password is already stored; pass --overwrite to replace it".into(),
                ));
            }
            let secret = args.source.read(None)?;
            creds.set(KEYCHAIN_ACCOUNT, &secret)?;
            if !cli.common.quiet {
                eprintln!("password stored in the OS keychain ({})", creds.service());
            }
            Ok(())
        }
    }
}

/// Exchange the stored password for a bearer token and cache it.
///
/// Unlike the AppFolio siblings there is no two-factor step on this path: the
/// portal offers an emailed one-time code as an *alternative* to the password,
/// not a second factor on top of it.
fn login(
    cli: &Cli,
    args: &LoginArgs,
    creds: &CredentialStore,
    cfg: &Config,
) -> Result<(), CliError> {
    let email = cfg.username().ok_or_else(|| {
        CliError::Usage(
            "no portal email configured — run `cpmfl config set username <you@example.com>`".into(),
        )
    })?;

    let password = match creds.get(KEYCHAIN_ACCOUNT)? {
        Some(p) if !args.overwrite => p,
        _ => {
            let prompt = if args.non_interactive {
                None
            } else {
                Some("Portal password")
            };
            let secret = args.source.read(prompt)?;
            creds.set(KEYCHAIN_ACCOUNT, &secret)?;
            secret
        }
    };

    let auth = Portal::authenticate(cfg, &email, &password)?;

    // Prove the token actually reads before caching it, so a login that can't
    // fetch anything leaves no broken session behind.
    if !args.no_verify {
        let portal = Portal::with_token(cfg, pk_cli_secrets::Secret::new(auth.token.expose()))?;
        portal.get(config::Service::Pay, "/account", &[])?;
    }

    creds.set(TOKEN_ACCOUNT, &auth.token)?;
    if !cli.common.quiet {
        let who = auth.user_name.as_deref().unwrap_or(email.as_str());
        eprintln!(
            "logged in as {who}; token cached in the OS keychain ({})",
            creds.service()
        );
    }
    Ok(())
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn config_cmd(cli: &Cli, cmd: &ConfigCmd, store: &ConfigStore) -> Result<(), CliError> {
    match cmd {
        ConfigCmd::Path => {
            println!("{}", store.path()?.display());
            Ok(())
        }
        ConfigCmd::Show => {
            let cfg: Config = store.load()?;
            let v = serde_json::to_value(&cfg).unwrap_or_default();
            if cli.common.json {
                output::json(&v);
            } else {
                output::render(&v);
            }
            Ok(())
        }
        ConfigCmd::Set { key, value } => {
            let mut cfg: Config = store.load()?;
            match key.as_str() {
                "users_url" => cfg.users_url = Some(value.clone()),
                "associations_url" => cfg.associations_url = Some(value.clone()),
                "actionitems_url" => cfg.actionitems_url = Some(value.clone()),
                "pay_url" => cfg.pay_url = Some(value.clone()),
                "username" => cfg.username = Some(value.clone()),
                "association_id" => {
                    cfg.association_id = Some(value.parse().map_err(|_| {
                        CliError::Usage(format!("association_id must be a number, got {value:?}"))
                    })?)
                }
                other => return Err(unknown_key(other)),
            }
            store.save(&cfg)
        }
        ConfigCmd::Unset { key } => {
            let mut cfg: Config = store.load()?;
            match key.as_str() {
                "users_url" => cfg.users_url = None,
                "associations_url" => cfg.associations_url = None,
                "actionitems_url" => cfg.actionitems_url = None,
                "pay_url" => cfg.pay_url = None,
                "username" => cfg.username = None,
                "association_id" => cfg.association_id = None,
                other => return Err(unknown_key(other)),
            }
            store.save(&cfg)
        }
    }
}

fn unknown_key(key: &str) -> CliError {
    CliError::Usage(format!(
        "unknown config key `{key}` (known: {})",
        config::KNOWN_KEYS.join(", ")
    ))
}
