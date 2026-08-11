//! ISO date handling at the CLI boundary.
//!
//! The portal speaks ISO-8601 *datetimes* (`2026-08-01T00:00:00`) while the
//! CLI contract is plain ISO dates (`2026-08-01`), so every date that reaches
//! a DTO goes through [`iso_date`].
//!
//! Range filtering is **client-side on purpose**. The Vantaca endpoints ignore
//! unrecognized query parameters silently — `?startDate=…` on `/Ledger`
//! returns the full unfiltered set with a 200 — so pushing `--since`/`--until`
//! at the server would look like it worked while quietly returning everything.
//! See `docs/api.md`.

use pk_cli_core::{dates, CliError};

/// Normalize a portal timestamp to an ISO `YYYY-MM-DD` date.
///
/// Returns `None` for a value that isn't a date at all, so a caller can omit
/// the field rather than emit a bogus one.
pub fn iso_date(raw: &str) -> Option<String> {
    let head = raw.split(['T', ' ']).next()?.trim();
    if head.is_empty() {
        return None;
    }
    // Validate through the shared parser so malformed provider data can't leak
    // into a DTO field the contract says is an ISO date.
    dates::parse_iso(head).ok().map(dates::fmt_iso)
}

/// Whether an ISO date falls within an inclusive `[since, until]` window.
/// Absent bounds are open; an unparseable date is kept rather than dropped, so
/// a filter never silently hides a record the CLI failed to understand.
pub fn in_range(date: Option<&str>, since: Option<&str>, until: Option<&str>) -> bool {
    let Some(d) = date else { return true };
    if let Some(s) = since {
        if d < s {
            return false;
        }
    }
    if let Some(u) = until {
        if d > u {
            return false;
        }
    }
    true
}

/// Validate the `--since` / `--until` pair before any network or keychain work.
pub fn validate_range(since: Option<&str>, until: Option<&str>) -> Result<(), CliError> {
    let s = since.map(dates::parse_iso).transpose()?;
    let u = until.map(dates::parse_iso).transpose()?;
    if let (Some(a), Some(b)) = (s, u) {
        if a > b {
            return Err(CliError::Usage(format!(
                "--since {} is after --until {}",
                since.unwrap_or_default(),
                until.unwrap_or_default()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_the_time_component() {
        assert_eq!(
            iso_date("2026-08-01T00:00:00").as_deref(),
            Some("2026-08-01")
        );
        assert_eq!(
            iso_date("2026-08-01T13:45:12.345Z").as_deref(),
            Some("2026-08-01")
        );
        assert_eq!(iso_date("2026-08-01").as_deref(), Some("2026-08-01"));
        assert_eq!(iso_date("2026-08-01 13:45").as_deref(), Some("2026-08-01"));
    }

    #[test]
    fn normalizes_loose_dates() {
        assert_eq!(iso_date("2026-8-1T00:00:00").as_deref(), Some("2026-08-01"));
    }

    #[test]
    fn rejects_non_dates() {
        for bad in ["", "   ", "not-a-date", "08/01/2026", "2026-13-01"] {
            assert_eq!(iso_date(bad), None, "{bad} should not parse");
        }
    }

    #[test]
    fn range_bounds_are_inclusive() {
        assert!(in_range(Some("2026-08-01"), Some("2026-08-01"), None));
        assert!(in_range(Some("2026-08-01"), None, Some("2026-08-01")));
        assert!(!in_range(Some("2026-07-31"), Some("2026-08-01"), None));
        assert!(!in_range(Some("2026-08-02"), None, Some("2026-08-01")));
    }

    #[test]
    fn open_bounds_and_undated_records_pass() {
        assert!(in_range(Some("1999-01-01"), None, None));
        assert!(in_range(None, Some("2026-08-01"), Some("2026-08-02")));
    }

    #[test]
    fn validate_rejects_bad_and_inverted_ranges() {
        assert!(validate_range(Some("2026-01-01"), Some("2026-02-01")).is_ok());
        assert!(validate_range(None, None).is_ok());
        assert!(matches!(
            validate_range(Some("2026-06-01"), Some("2026-01-01")),
            Err(CliError::Usage(_))
        ));
        assert!(validate_range(Some("06/01/2026"), None).is_err());
        assert!(validate_range(None, Some("nope")).is_err());
    }
}
