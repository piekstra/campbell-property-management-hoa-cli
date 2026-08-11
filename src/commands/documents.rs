//! `cpmfl documents` — the association's shared document library.
//!
//! The library is a folder tree: `list` walks it one level at a time and
//! `search` queries the whole thing. A file row carries a `documentUrl` — a
//! short-lived Azure blob SAS URL — which `list`/`search` emit already
//! percent-encoded so it is directly fetchable (see [`fetchable_url`]).
//! `download` skips the SAS URL entirely and streams the bytes through the
//! portal's own `Documents/{id}` endpoint.

use std::path::{Path, PathBuf};

use pk_cli_core::{output, CliError};
use serde_json::{json, Value};

use super::{emit, emit_list, limited, table_view, Ctx};
use crate::config::Service;
use crate::dates::iso_date;

#[derive(clap::Subcommand, Debug)]
pub enum Cmd {
    /// List a folder in the document library (default: the root).
    List {
        /// Folder id, as reported by a previous `list`.
        #[arg(long, value_name = "ID")]
        folder: Option<u64>,
        /// Maximum entries to return.
        #[arg(long, value_name = "N")]
        limit: Option<u32>,
    },
    /// Search the whole library by name.
    Search {
        /// Text to match against document names.
        term: String,
        /// Page of results (1-based).
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// Maximum entries to return.
        #[arg(long, value_name = "N")]
        limit: Option<u32>,
    },
    /// Download a document's bytes to a local file.
    Download {
        /// Document id, as reported by `list` or `search`.
        id: u64,
        /// Where to write the file.
        #[arg(long, short, value_name = "PATH")]
        output: PathBuf,
    },
}

pub fn run(ctx: &Ctx, cmd: &Cmd) -> Result<(), CliError> {
    match cmd {
        Cmd::List { folder, limit } => list(ctx, *folder, *limit),
        Cmd::Search { term, page, limit } => search(ctx, term, *page, *limit),
        Cmd::Download { id, output } => download(ctx, *id, output),
    }
}

fn list(ctx: &Ctx, folder: Option<u64>, limit: Option<u32>) -> Result<(), CliError> {
    let client = ctx.client()?;
    let association = ctx.association_id(&client)?;
    // Navigation is `Directories?parentId=`, not `Directories/{id}`. The
    // portal ignores unknown query parameters silently, so a wrong spelling
    // here returns the *root* listing with a 200 rather than an error.
    // `Documents/{id}` is a different thing entirely: the file download.
    let query = match folder {
        Some(id) => vec![("parentId", id.to_string())],
        None => vec![],
    };
    let body = client.get(
        Service::Associations,
        &format!("/associations/{association}/Directories"),
        &query,
    )?;

    let raw = body
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let items = limited(raw.iter().map(entry).collect(), limit);

    if ctx.common.json {
        let breadcrumbs = body.get("breadcrumbs").cloned().unwrap_or(Value::Null);
        let mut paged = pk_cli_utility::Paged::new("document", items);
        paged.total = Some(raw.len() as u64);
        let mut v = serde_json::to_value(&paged).unwrap_or(Value::Null);
        if let Value::Object(ref mut m) = v {
            m.insert("breadcrumbs".into(), breadcrumbs);
        }
        output::json(&v);
    } else {
        output::table(&table_view(
            &items,
            &["id", "type", "name", "last_modified", "visibility"],
        ));
    }
    Ok(())
}

fn search(ctx: &Ctx, term: &str, page: u32, limit: Option<u32>) -> Result<(), CliError> {
    if term.trim().is_empty() {
        return Err(CliError::Usage("search term cannot be empty".into()));
    }
    let client = ctx.client()?;
    let association = ctx.association_id(&client)?;
    let body = client.get(
        Service::Associations,
        &format!("/associations/{association}/Documents/Search"),
        &[("search", term.to_string()), ("page", page.to_string())],
    )?;

    // Search answers with its own envelope (`documents` / `totalResults`)
    // rather than the `member` / `totalItems` one the rest of the API uses.
    let raw = body
        .get("documents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let total = body.get("totalResults").and_then(Value::as_u64);
    let items = limited(raw.iter().map(entry).collect(), limit);

    if items.is_empty() && page == 1 {
        return Err(CliError::NotFound(format!(
            "no documents matching {term:?}"
        )));
    }

    if ctx.common.json {
        emit_list(ctx, "document", items, total);
    } else {
        output::table(&table_view(
            &items,
            &["id", "type", "name", "last_modified", "path"],
        ));
    }
    Ok(())
}

/// Fetch a document's bytes through the portal (`Documents/{id}`) and write
/// them to `output`. Still a read: nothing on the portal changes. Going
/// through the portal rather than the row's SAS URL means the caller never
/// handles that URL at all — it expires within minutes anyway.
fn download(ctx: &Ctx, id: u64, output: &Path) -> Result<(), CliError> {
    let client = ctx.client()?;
    let association = ctx.association_id(&client)?;
    let (bytes, content_type) = client.get_bytes(
        Service::Associations,
        &format!("/associations/{association}/Documents/{id}"),
    )?;
    // A 200 with no body means the id resolved to something that isn't a
    // downloadable file (a folder, say). An empty file would look like success.
    if bytes.is_empty() {
        return Err(CliError::NotFound(format!(
            "document {id} has no content — is it a folder id?"
        )));
    }
    std::fs::write(output, &bytes)
        .map_err(|e| CliError::Other(format!("writing {}: {e}", output.display())))?;

    let payload = json!({
        "id": id,
        "path": output.display().to_string(),
        "bytes": bytes.len(),
        "content_type": content_type,
    });
    emit(ctx, "document-download", payload, |_| {
        if !ctx.common.quiet {
            eprintln!(
                "saved document {id} to {} ({} bytes)",
                output.display(),
                bytes.len()
            );
        }
    });
    Ok(())
}

/// Normalize a library row. Folders and files share a shape; `type` tells them
/// apart and only files carry a `documentUrl`.
fn entry(d: &Value) -> Value {
    json!({
        "id": d.get("id"),
        "type": d.get("type"),
        "name": d.get("name"),
        "last_modified": d
            .get("lastModified")
            .and_then(Value::as_str)
            .and_then(iso_date),
        "visibility": d.get("visibility"),
        "path": d.get("fullPath").filter(|v| v.as_str() != Some("")),
        "url": d
            .get("documentUrl")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(fetchable_url),
    })
}

/// Make a portal-issued URL directly fetchable.
///
/// The portal builds its blob SAS URLs from document file names verbatim, so
/// the path can contain literal spaces — illegal in a request-target, and
/// enough to make `curl`/`urllib` reject or mangle the URL. But the URL also
/// already carries `%xx` escapes (`%3B` for `;`, and the whole signed SAS
/// query), so re-encoding it wholesale turns those into `%25xx` and breaks the
/// SAS signature with a 403. The safe transform is the surgical one: encode
/// *only* bytes that can never appear raw in a URL, and leave everything else
/// — `%` included — exactly as issued.
fn fetchable_url(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for &b in url.as_bytes() {
        let invalid = b <= 0x20 // control characters and space
            || b >= 0x7f // DEL, and the bytes of any non-ASCII character
            || matches!(b, b'"' | b'<' | b'>' | b'\\' | b'^' | b'`' | b'{' | b'|' | b'}');
        if invalid {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push(b as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_normalizes_dates_and_drops_empty_strings() {
        let row = entry(&json!({
            "id": 7,
            "type": "File",
            "name": "Budget.pdf",
            "lastModified": "2026-08-01T12:00:00",
            "visibility": "Public",
            "documentUrl": "",
            "fullPath": ""
        }));
        assert_eq!(row["last_modified"], json!("2026-08-01"));
        // Empty provider strings become absent fields, not empty ones.
        assert_eq!(row["url"], Value::Null);
        assert_eq!(row["path"], Value::Null);
    }

    #[test]
    fn entry_keeps_real_urls() {
        let row = entry(&json!({ "documentUrl": "https://example.test/a.pdf" }));
        assert_eq!(row["url"], json!("https://example.test/a.pdf"));
    }

    /// The bug this module exists to not have: a blob path built from a file
    /// name with spaces must come out fetchable.
    #[test]
    fn entry_emits_fetchable_urls() {
        let row = entry(&json!({
            "documentUrl": "https://example.test/docs/2022 Recorded Declaration.pdf?sv=1&sig=abc"
        }));
        assert_eq!(
            row["url"],
            json!("https://example.test/docs/2022%20Recorded%20Declaration.pdf?sv=1&sig=abc")
        );
    }

    #[test]
    fn fetchable_url_encodes_only_invalid_bytes() {
        // Spaces become %20; everything already legal is untouched.
        assert_eq!(
            fetchable_url("https://h/a b c.pdf"),
            "https://h/a%20b%20c.pdf"
        );
        // Existing escapes must survive verbatim — double-encoding `%3B` into
        // `%253B` is exactly what invalidates a SAS signature.
        assert_eq!(
            fetchable_url("https://h/Covenants%3B Restated.pdf?sig=a%2Fb%3D"),
            "https://h/Covenants%3B%20Restated.pdf?sig=a%2Fb%3D"
        );
        // A fully valid URL is a fixed point.
        let clean = "https://h/dir/file.pdf?sv=2024-01-01&sig=x%2By&se=2026-01-01T00%3A00%3A00Z";
        assert_eq!(fetchable_url(clean), clean);
        // The other request-target-illegal characters are encoded too.
        assert_eq!(
            fetchable_url("https://h/a\"<>\\^`{|}"),
            "https://h/a%22%3C%3E%5C%5E%60%7B%7C%7D"
        );
        // Non-ASCII encodes per UTF-8 byte.
        assert_eq!(
            fetchable_url("https://h/café.pdf"),
            "https://h/caf%C3%A9.pdf"
        );
    }
}
