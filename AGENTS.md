# AGENTS.md

Guidance for AI coding agents (and humans) working in this repo. Tool-agnostic;
`CLAUDE.md` points here.

## What this is

`cpmfl` — a Rust CLI for the **Vantaca** homeowner portal that Campbell
Property Management runs at `portal.campbellproperty.com`. A thin,
portal-specific layer over the shared
[`cli-common`](https://github.com/piekstra/cli-common) `pk-cli-*` crates
(auth, http, config, secrets, self-update, utility profile). This repo owns
only the portal client, the commands, and their DTOs.

There is no official API. Everything targets the undocumented JSON endpoints
the portal's own front end calls, mapped by watching its XHR traffic and
reading its bundles, and written up in [`docs/api.md`](docs/api.md).

## Build, test, lint

```console
make verify     # fmt-check + clippy -D warnings + tests + smoke — the CI gate
make test       # unit + integration (fully offline; no network, no creds)
make dev        # debug build, re-signed so keychain grants survive rebuilds
cargo run -- summary
```

Run `make verify` before considering a change done — it's exactly what CI runs.

## Layout

- `src/main.rs` — clap command tree, the login flow, exit-code mapping.
- `src/client.rs` — the portal HTTP client: bearer-token auth, the local JWT
  expiry check, and the status→exit-code mapping. Its module doc explains the
  auth model and why `Origin` is load-bearing.
- `src/commands/*.rs` — one module per domain; `money.rs` holds the ledger
  side, `community.rs` the non-money reads. Each renders a human table and a
  `--json` DTO.
- `src/config.rs` — non-secret config and the four service base URLs; every
  secret is keychain-only.
- `src/dates.rs` — ISO datetime → ISO date, plus range validation/filtering.
- `tests/` — offline surface + contract tests, and `tests/fixtures/` (read its
  README before touching a fixture).

## Conventions (do not break these)

- **`--json` on every command**, emitting one DTO tagged with a `schema` field
  (e.g. `"schema":"transaction-list/v1"`). Human output → stdout as a table;
  diagnostics → stderr. Keep the two paths in sync; a breaking DTO change bumps
  the `/vN` suffix.
- **Exit codes:** 0 ok · 2 usage · 3 auth · 4 not found · 5 upstream · 6
  confirmation required. Validate args **before** touching the keychain or
  network, so `--help` and bad args never prompt, hang, or hit the portal.
- **Read-only.** Every command observes. The portal supports payments, autopay
  enrollment, request submission, amenity booking, and profile edits; none are
  implemented, and `docs/api.md` catalogs all 83 write-capable endpoints. If a
  write is ever added it must prompt for confirmation and require `--force`
  non-interactively (exit 6 otherwise) — and this section must stop saying
  "read-only". `the_surface_carries_no_mutating_commands` and
  `api_passthrough_exposes_no_write_method` in `tests/cli_surface.rs` enforce
  this mechanically.
- **Secrets** come from the OS keychain or stdin — never argv, never logs,
  never a file in the repo. Service `piekstra.cpmfl`, accounts `password`,
  `token`.
- **Dates** are ISO `YYYY-MM-DD` at the CLI boundary; the portal's ISO
  datetimes are truncated in `dates.rs`.

## Portal-specific gotchas

- **Unknown query parameters are silently ignored.** `GET /Ledger?startDate=…`
  returns **200 with the full unfiltered set**. Nothing signals the filter was
  dropped. This is why `--since`/`--until` filter client-side, and why
  directory navigation must be spelled `Directories?parentId=` exactly —
  `folderId`, `directoryId`, and `id` all return the *root* listing with a 200.
  Never "optimize" a filter into a query parameter without proving the server
  honours it.
- **Money scale differs by endpoint.** `/Ledger` returns integer **cents**;
  `/Payment`, `/Charge`, `/account` return **dollars** — for the same
  transaction. `commands::money` vs `commands::money_cents` keep this explicit
  at each call site. A fixture test asserts the 100× relationship; don't
  "fix" it.
- **`Origin` identifies the tenant.** Vantaca's API hosts are shared across
  management companies, so `/Users/Authenticate` fails with "Invalid company"
  without the right `Origin`. Every request sends it.
- **403 usually means "not enabled here", not "log in again".** The client maps
  it to `NotFound` (exit 4) so users aren't sent into a re-login loop.
  Association-scoped board endpoints answer **500** for a non-board login;
  treat that as "not permitted", not an outage.
- **`/Users/Authenticate` is the only POST** the CLI makes, and the only place
  a credential is sent. There is no two-factor step on the password path.
- `documents download` uses `Documents/{id}` (a blob), which is a *different*
  endpoint from folder navigation. Don't conflate them. The `documentUrl` in
  list/search rows is a short-lived SAS URL whose signature breaks if the URL
  is re-encoded — `commands::documents::fetchable_url` explains the rules.

## Safety & privacy (written as if this repo were public)

- Never commit a password, bearer token, real name, address, balance, account
  number, or association name.
- `tests/fixtures/` are **scrubbed** captures: structure preserved exactly,
  every identifying and financial value replaced with an obvious dummy. The
  policy is in `tests/fixtures/README.md`, and
  `fixtures_carry_no_real_identifiers` in `tests/fixture_shapes.rs` enforces
  it — as an allow-list (each identity-bearing field must equal a known dummy),
  not a deny-list of real values, since a deny-list would itself commit the
  identifiers it exists to exclude. Add the field's dummy to `EXPECTED` when a
  new identity-bearing key appears.
- Document `documentUrl`s point at live association documents in blob storage
  — never commit one.
- Don't paste real portal output into an issue, commit message, or doc example.
  The README's examples use scrubbed figures.

## Definition of done

`make verify` green, tests cover the change, `--json` and human output both
updated, `docs/api.md` still matches reality, and no secrets or PII anywhere in
the diff — including fixtures.
