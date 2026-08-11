//! HTTP client for the Vantaca homeowner portal behind
//! `portal.campbellproperty.com`.
//!
//! Vantaca publishes no homeowner API. Everything here targets the same JSON
//! endpoints the portal's own front end calls, mapped by watching its XHR
//! traffic and reading its bundles. See `docs/api.md`.
//!
//! # Auth model
//!
//! `POST /Users/Authenticate` with `{userName, password}` returns a JWT. Every
//! read then sends `Authorization: Bearer <jwt>`; there is no cookie session,
//! no CSRF token, and no two-factor step on the password path. That makes this
//! CLI markedly simpler than its AppFolio siblings: the token is the session.
//!
//! The JWT carries a standard `exp` claim, so an expired token is detectable
//! *before* spending a request — [`Portal::from_cached_token`] checks it and
//! fails with [`CliError::Auth`] pointing at `cpmfl auth login`. The server is
//! still the authority; the local check only turns a confusing 401 into a
//! clear one.
//!
//! # Service split
//!
//! Vantaca fans the portal out over four microservices ([`Service`]). A path
//! is meaningless without knowing which one serves it, so every read names its
//! service and the client resolves the base URL from [`Config`].
//!
//! There is a second, unrelated credential in this ecosystem: the "Pay Now"
//! button hands off to Western Alliance Bank
//! (`pay.westernalliancebank.com`) with its own login. That surface is not
//! part of this CLI.

use pk_cli_auth::token as jwt;
use pk_cli_core::CliError;
use pk_cli_secrets::{CredentialStore, Secret};
use serde_json::Value;

use crate::config::{Config, Service, TOKEN_ACCOUNT};

const BIN: &str = "cpmfl";

/// What a successful password login yields.
pub struct Authenticated {
    pub token: Secret,
    pub user_name: Option<String>,
}

/// An authenticated portal session.
pub struct Portal {
    http: reqwest::blocking::Client,
    cfg: Config,
    token: Secret,
}

impl Portal {
    /// Exchange a password for a bearer token. This is the only call in the
    /// CLI that sends a credential, and the only POST it makes.
    pub fn authenticate(
        cfg: &Config,
        username: &str,
        password: &Secret,
    ) -> Result<Authenticated, CliError> {
        let http = pk_cli_http::client(BIN, env!("CARGO_PKG_VERSION"))?;
        let url = format!("{}/Users/Authenticate", cfg.base_url(Service::Users));
        let origin = cfg.portal_url();
        let resp = http
            .post(&url)
            .header(reqwest::header::ORIGIN, &origin)
            .header(reqwest::header::REFERER, format!("{origin}/"))
            .json(&serde_json::json!({
                "userName": username,
                "password": password.expose(),
            }))
            .send()
            .map_err(|e| CliError::Upstream(format!("contacting the portal: {e}")))?;

        let status = resp.status().as_u16();
        let body: Value = read_json(resp)?;

        if status == 400 || status == 401 {
            return Err(CliError::Auth(format!(
                "the portal rejected those credentials ({})",
                provider_message(&body).unwrap_or_else(|| format!("HTTP {status}"))
            )));
        }
        if !(200..300).contains(&status) {
            return Err(CliError::Upstream(provider_error(status, &body)));
        }

        // A 200 with `isValid: false` is the portal's soft rejection — an
        // account that authenticated but can't be used (locked, or awaiting a
        // forced password reset). Treat it as an auth failure, not success.
        if body.get("isValid").and_then(Value::as_bool) == Some(false) {
            let hint = if body.get("shouldResetPassword").and_then(Value::as_bool) == Some(true) {
                " — the portal wants the password reset before this account can be used"
            } else {
                ""
            };
            return Err(CliError::Auth(format!(
                "the portal refused the login{hint}"
            )));
        }

        let token = body
            .get("token")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                CliError::Upstream("login succeeded but the portal issued no token".into())
            })?;

        Ok(Authenticated {
            token: Secret::new(token),
            user_name: body
                .get("userName")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    /// Replay the cached bearer token from the keychain.
    pub fn from_cached_token(cfg: &Config, creds: &CredentialStore) -> Result<Portal, CliError> {
        let token = creds
            .get(TOKEN_ACCOUNT)?
            .ok_or_else(|| CliError::Auth("not logged in — run `cpmfl auth login`".into()))?;
        if jwt::is_expired(token.expose(), now_unix(), jwt::DEFAULT_SKEW_SECS) {
            return Err(CliError::Auth(
                "the cached portal session has expired — run `cpmfl auth login`".into(),
            ));
        }
        Portal::with_token(cfg, token)
    }

    pub fn with_token(cfg: &Config, token: Secret) -> Result<Portal, CliError> {
        Ok(Portal {
            http: pk_cli_http::client(BIN, env!("CARGO_PKG_VERSION"))?,
            cfg: cfg.clone(),
            token,
        })
    }

    /// A read against one of the portal's services.
    pub fn get(
        &self,
        service: Service,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Value, CliError> {
        let resp = self
            .request(service, path)
            .query(query)
            .send()
            .map_err(|e| CliError::Upstream(format!("contacting the portal: {e}")))?;
        self.interpret(resp)
    }

    /// A raw read: the response bytes and content type rather than parsed
    /// JSON. Backs `documents download`, where the portal streams a blob.
    pub fn get_bytes(
        &self,
        service: Service,
        path: &str,
    ) -> Result<(Vec<u8>, Option<String>), CliError> {
        let resp = self
            .request(service, path)
            .send()
            .map_err(|e| CliError::Upstream(format!("contacting the portal: {e}")))?;

        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            // Even the blob endpoint reports errors as the JSON envelope; if
            // this one didn't, fall back to the bare status line.
            let body = read_json(resp).unwrap_or(Value::Null);
            return Err(status_error(status, &body));
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let bytes = resp
            .bytes()
            .map_err(|e| CliError::Upstream(format!("reading the portal response: {e}")))?
            .to_vec();
        Ok((bytes, content_type))
    }

    /// The GET every read shares: service base URL + path, bearer token, and
    /// the tenant-identifying `Origin`/`Referer` pair.
    fn request(&self, service: Service, path: &str) -> reqwest::blocking::RequestBuilder {
        let url = format!(
            "{}/{}",
            self.cfg.base_url(service).trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let origin = self.cfg.portal_url();
        self.http
            .get(&url)
            .bearer_auth(self.token.expose())
            .header(reqwest::header::ORIGIN, &origin)
            .header(reqwest::header::REFERER, format!("{origin}/"))
    }

    /// Parse a JSON response, mapping non-2xx onto the exit-code contract.
    fn interpret(&self, resp: reqwest::blocking::Response) -> Result<Value, CliError> {
        let status = resp.status().as_u16();
        let body = read_json(resp)?;
        if (200..300).contains(&status) {
            Ok(body)
        } else {
            Err(status_error(status, &body))
        }
    }
}

/// Map a non-2xx status onto the family's exit-code contract, preferring the
/// portal's own `message`/`detail` over a bare status line.
fn status_error(status: u16, body: &Value) -> CliError {
    match status {
        401 => CliError::Auth("the portal rejected the session — run `cpmfl auth login`".into()),
        // 403 is a capability answer here, not an expired session: the
        // portal returns it for features an association hasn't enabled
        // (e.g. amenity reservations). Surfacing it as Auth would send
        // users into a pointless re-login loop.
        403 => CliError::NotFound(
            provider_message(body).unwrap_or_else(|| "not available for this association".into()),
        ),
        404 => {
            CliError::NotFound(provider_message(body).unwrap_or_else(|| "no such record".into()))
        }
        400 => CliError::Usage(
            provider_message(body).unwrap_or_else(|| "the portal rejected the request".into()),
        ),
        _ => CliError::Upstream(provider_error(status, body)),
    }
}

/// Parse a response body as JSON, tolerating an empty one (204s and some
/// portal errors send nothing at all).
fn read_json(resp: reqwest::blocking::Response) -> Result<Value, CliError> {
    let text = resp
        .text()
        .map_err(|e| CliError::Upstream(format!("reading the portal response: {e}")))?;
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text)
        .map_err(|e| CliError::Upstream(format!("parsing the portal response as JSON: {e}")))
}

/// The portal's error envelope is `{title, status, detail, errorId, errorCode,
/// message}`. `message` and `detail` are the human-readable ones.
fn provider_message(body: &Value) -> Option<String> {
    for key in ["message", "detail", "title"] {
        if let Some(s) = body.get(key).and_then(Value::as_str) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn provider_error(status: u16, body: &Value) -> String {
    match provider_message(body) {
        Some(m) => format!("the portal returned HTTP {status}: {m}"),
        None => format!("the portal returned HTTP {status}"),
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_message_prefers_message_then_detail() {
        assert_eq!(
            provider_message(&json!({ "message": "m", "detail": "d", "title": "t" })),
            Some("m".into())
        );
        assert_eq!(
            provider_message(&json!({ "detail": "d", "title": "t" })),
            Some("d".into())
        );
        assert_eq!(provider_message(&json!({ "title": "t" })), Some("t".into()));
        assert_eq!(provider_message(&json!({})), None);
        // Empty strings are not messages.
        assert_eq!(provider_message(&json!({ "message": "" })), None);
    }

    /// The status → exit-code contract, exercised through the shared mapper
    /// that both the JSON and the blob paths use.
    #[test]
    fn status_error_maps_onto_the_exit_contract() {
        assert_eq!(status_error(401, &json!({})).exit_code(), 3);
        assert_eq!(status_error(403, &json!({})).exit_code(), 4);
        assert_eq!(status_error(404, &json!({})).exit_code(), 4);
        assert_eq!(status_error(400, &json!({})).exit_code(), 2);
        assert_eq!(status_error(500, &json!({})).exit_code(), 5);
    }

    #[test]
    fn provider_error_includes_status_and_message() {
        assert_eq!(
            provider_error(500, &json!({ "message": "boom" })),
            "the portal returned HTTP 500: boom"
        );
        assert_eq!(
            provider_error(502, &json!({})),
            "the portal returned HTTP 502"
        );
    }
}
