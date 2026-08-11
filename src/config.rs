//! Non-secret settings (`~/.config/cpmfl/config.json`).
//!
//! The portal password and the cached bearer token live in the OS keychain
//! (service `piekstra.cpmfl`), never here.

use serde::{Deserialize, Serialize};

/// Vantaca splits the portal across per-domain microservices. A homeowner read
/// touches four of them, so the client resolves a base per [`Service`] rather
/// than hanging everything off one host.
pub const DEFAULT_USERS_URL: &str = "https://vantaca-api-users-prod-001.vantaca.net/api";
pub const DEFAULT_ASSOCIATIONS_URL: &str =
    "https://vantaca-api-associations-prod-001.vantaca.net/api";
pub const DEFAULT_ACTIONITEMS_URL: &str =
    "https://vantaca-api-actionitems-prod-001.vantaca.net/api";
pub const DEFAULT_PAY_URL: &str = "https://api-pay-platform.vantaca.net/api";

/// The management company's portal front end.
///
/// Vantaca is multi-tenant and the shared API hosts carry no company in their
/// URL, so the *only* thing identifying which management company a login
/// belongs to is the `Origin` header. Send the wrong one (or none) and
/// `/Users/Authenticate` fails with "Invalid company" even for a valid
/// password. Overridable via `config set portal_url` so the CLI can point at
/// another Vantaca-hosted company.
pub const DEFAULT_PORTAL_URL: &str = "https://portal.campbellproperty.com";

/// Keychain account the portal password is stored under.
pub const KEYCHAIN_ACCOUNT: &str = "password";

/// Keychain account for the cached bearer token. Every read authenticates with
/// this JWT alone, so caching it is what lets ordinary commands run without a
/// fresh password round-trip.
pub const TOKEN_ACCOUNT: &str = "token";

/// Which Vantaca microservice a request targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    /// Identity, authentication, profile configuration.
    Users,
    /// Association-scoped reads: documents, board directory, calendar.
    Associations,
    /// Homeowner requests, amenities, forms, work orders.
    ActionItems,
    /// Ledger, payments, charges, autopay, payment methods.
    Pay,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Override the users-service base URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub users_url: Option<String>,

    /// Override the associations-service base URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub associations_url: Option<String>,

    /// Override the action-items-service base URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actionitems_url: Option<String>,

    /// Override the payments-platform base URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pay_url: Option<String>,

    /// Override the portal front-end origin that identifies the management
    /// company (see [`DEFAULT_PORTAL_URL`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portal_url: Option<String>,

    /// Portal login email (identity label only; secrets stay in the keychain).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// Association to scope association-level reads to. Learned at login from
    /// the account record, so it rarely needs setting by hand.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub association_id: Option<u64>,
}

impl Config {
    pub fn base_url(&self, service: Service) -> String {
        match service {
            Service::Users => self
                .users_url
                .clone()
                .unwrap_or_else(|| DEFAULT_USERS_URL.into()),
            Service::Associations => self
                .associations_url
                .clone()
                .unwrap_or_else(|| DEFAULT_ASSOCIATIONS_URL.into()),
            Service::ActionItems => self
                .actionitems_url
                .clone()
                .unwrap_or_else(|| DEFAULT_ACTIONITEMS_URL.into()),
            Service::Pay => self
                .pay_url
                .clone()
                .unwrap_or_else(|| DEFAULT_PAY_URL.into()),
        }
    }

    /// The origin identifying the management company.
    pub fn portal_url(&self) -> String {
        self.portal_url
            .clone()
            .unwrap_or_else(|| DEFAULT_PORTAL_URL.into())
    }

    /// Resolve the login email: config, then `$CPMFL_USERNAME`.
    pub fn username(&self) -> Option<String> {
        self.username.clone().or_else(|| {
            std::env::var("CPMFL_USERNAME")
                .ok()
                .filter(|s| !s.is_empty())
        })
    }
}

/// Config keys settable via `cpmfl config set <key> <value>`.
pub const KNOWN_KEYS: &[&str] = &[
    "users_url",
    "associations_url",
    "actionitems_url",
    "pay_url",
    "portal_url",
    "username",
    "association_id",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_urls_default_per_service() {
        let cfg = Config::default();
        assert_eq!(cfg.base_url(Service::Users), DEFAULT_USERS_URL);
        assert_eq!(cfg.base_url(Service::Pay), DEFAULT_PAY_URL);
        assert_eq!(
            cfg.base_url(Service::Associations),
            DEFAULT_ASSOCIATIONS_URL
        );
        assert_eq!(cfg.base_url(Service::ActionItems), DEFAULT_ACTIONITEMS_URL);
    }

    #[test]
    fn overrides_apply_only_to_their_service() {
        let cfg = Config {
            pay_url: Some("https://pay.test".into()),
            ..Default::default()
        };
        assert_eq!(cfg.base_url(Service::Pay), "https://pay.test");
        assert_eq!(cfg.base_url(Service::Users), DEFAULT_USERS_URL);
    }
}
