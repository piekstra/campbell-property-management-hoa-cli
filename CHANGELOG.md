# Changelog

All notable changes to `cpmfl` are documented here. Newest first. The project follows
[SemVer](https://semver.org/); until 1.0, minors carry the breaking-change signal.

## 0.2.1 — 2026-11-03

- `cpmfl auth logout --forget` also removes the `mcp --http` token, and says what a running
  server and `$CPMFL_MCP_TOKEN` still need.
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
