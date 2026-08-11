//! `cpmfl api` — raw passthrough to any portal endpoint.
//!
//! The escape hatch for surfaces the typed commands don't cover yet, and the
//! quickest way to check whether the portal's JSON has drifted.
//!
//! GET only. The portal has plenty of write endpoints (`docs/api.md` catalogs
//! them); this CLI is read-only, and an `api` command that could POST would
//! quietly undo that. Adding writes means adding confirmation prompts and
//! `--force`, not widening this hatch.

use clap::Args;
use pk_cli_core::{output, CliError};

use super::Ctx;
use crate::config::Service;

#[derive(Args, Debug)]
pub struct ApiArgs {
    /// Which Vantaca service serves this path.
    #[arg(long, value_enum, default_value = "pay")]
    pub service: ServiceArg,

    /// Path under the service base, e.g. `/Ledger`.
    pub path: String,

    /// Repeatable query parameter, `key=value`.
    #[arg(long = "query", value_name = "KEY=VALUE")]
    pub query: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ServiceArg {
    /// Identity and profile (`vantaca-api-users-*`).
    Users,
    /// Association reads: documents, directory, calendar.
    Associations,
    /// Requests, amenities, forms (`vantaca-api-actionitems-*`).
    ActionItems,
    /// Ledger, payments, charges (`api-pay-platform`).
    Pay,
}

impl From<ServiceArg> for Service {
    fn from(a: ServiceArg) -> Service {
        match a {
            ServiceArg::Users => Service::Users,
            ServiceArg::Associations => Service::Associations,
            ServiceArg::ActionItems => Service::ActionItems,
            ServiceArg::Pay => Service::Pay,
        }
    }
}

pub fn run(ctx: &Ctx, args: &ApiArgs) -> Result<(), CliError> {
    // Parse before touching the keychain or the network, so a typo costs
    // nothing and never prompts.
    let mut query = Vec::new();
    for pair in &args.query {
        let (k, v) = pair
            .split_once('=')
            .ok_or_else(|| CliError::Usage(format!("--query expects KEY=VALUE, got {pair:?}")))?;
        query.push((k.to_string(), v.to_string()));
    }
    let borrowed: Vec<(&str, String)> =
        query.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();

    let payload = ctx
        .client()?
        .get(args.service.into(), &args.path, &borrowed)?;
    output::json(&payload);
    Ok(())
}
