# Changelog

All notable changes to `cpmfl` are documented here. Newest first. The project follows
[SemVer](https://semver.org/); until 1.0, minors carry the breaking-change signal.

## 0.2.3 — 2026-11-03

- `campbell_requests` pages by position, as `campbell_bills` and `campbell_payments` now do, so a row
  that arrives between pages repeats nothing and skips nothing. Rows of the same day now run in a
  fixed order of their own. A cursor from an earlier version still works, as the count of rows it
  holds. A request with no `id` is now an upstream error, since the id decides its position.
- A bill's `id` in `campbell_bills` is now its day and amount, so it stays the same when a charge
  posts. The portal's two ids for one bill would have moved it between pages.
- `auth logout --forget` attempts every step and reports every failure, where the first failure used
  to stop the rest. When it removed a connector token, it says a running `cpmfl mcp --http` keeps
  accepting that token until it stops, whatever else failed. When the keychain held none, it says so
  and names `$CPMFL_MCP_TOKEN`.
- On Windows, a session too long for one credential is split and read back as one.
- cli-common v0.27.0, up from v0.24.0.

## 0.2.2 — 2026-11-03

- Each service URL must be `https`, or `http` to this machine, before the password or the
  bearer token goes to it. A command and a tool refuse one off `https` before they read
  the keychain.
- A redirect that leaves `https` is an error, and the sign-in, which carries the password,
  follows a redirect only on the service's host.
- A network failure names the service's origin, not the whole URL, and says why a redirect
  was refused.
- cli-common v0.24.0, up from v0.22.0.

## 0.2.1 — 2026-11-03

- `cpmfl auth logout --forget` also removes the `mcp --http` token, and says what a running
  server and `$CPMFL_MCP_TOKEN` still need.
- The release action that holds the write token is pinned to a commit.
- cli-common v0.22.0.

## 0.2.0 — 2026-11-03

- `cpmfl mcp`: five read-only tools for Claude: `campbell_balance`, `campbell_bills`,
  `campbell_payments` (the `utility/v1` set from cli-common's `pk-cli-utility`),
  `campbell_requests` and `campbell_documents`. They run over stdio for Claude Code and
  Claude Desktop, and with `--http` on 127.0.0.1 behind a bearer token for Claude on the
  web and mobile through your own Cloudflare Worker and tunnel. No tool can make a
  payment, change autopay, submit a request or change anything at the portal.
- `cpmfl mcp` reads `$CPMFL_TOKEN` before the keychain. It never sends the password or
  writes the keychain, and reads the config and the token on each call. A changed
  portal answer is an error rather than a zero balance or an empty list. Every other
  command is unchanged.
- Releases attach `cpmfl-<version>.mcpb`, a Claude Desktop extension, and a Windows
  x86_64 tarball.
- `self-update` says to restart a running `cpmfl mcp --http` and to install the new
  `.mcpb`.
- cli-common v0.21.0, up from v0.15.1. Rust 1.89 or later.
- Fixtures: payment record ids are now dummies; the fixture README no longer describes
  the privacy test as a banned list.
