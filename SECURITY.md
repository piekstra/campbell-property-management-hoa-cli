# Security Policy

## Reporting a vulnerability

Please report security issues privately via GitHub's
[security advisories](https://github.com/piekstra/campbell-property-management-hoa-cli/security/advisories/new)
rather than a public issue.

## Threat model

`cpmfl` authenticates to a third-party HOA-management portal on behalf of
one homeowner and reads their account data. The things worth protecting:

- **The portal password**, stored in the OS keychain under service
  `piekstra.cpmfl`, account `password`. It is read at point of use, never
  logged, never placed on argv, and never written to disk.
- **The cached bearer token** (account `token`). It is a credential: anyone
  holding it can read the account until it expires. It lives in the keychain
  and is redacted from all output.
- **The `cpmfl mcp --http` bearer token** (account `mcp-http-token`, or
  `CPMFL_MCP_TOKEN`), only if you use it, with **a copy in your Cloudflare Worker** as
  its `HOME_CAMPBELL_TOKEN` secret. It admits a caller to the read-only tools. See
  [The MCP server](#the-mcp-server).
- **Document URLs.** The portal returns direct links to document bytes;
  `cpmfl documents get` prints one only when explicitly asked. Never paste
  one into an issue or commit it.

## What this tool does not do

- It sends no password or token over plain `http`, except to this machine: each service
  URL and every redirect must be `https`. The sign-in, which carries the password, follows
  a redirect only on the service's host.
- It never mutates the portal — no payments, approvals, or profile changes.
- It talks only to the configured Vantaca service hosts and the document
  links those hosts return. No telemetry, no third-party services, and no
  server component unless you run `cpmfl mcp --http`.
- It hardcodes no secrets. `gitleaks` runs in CI over the full history.

## The MCP server

`cpmfl mcp` serves read-only tools to an MCP client (Claude Code, Claude Desktop) over
stdio; it opens no port unless run with `--http` (below). **What the tools return goes
to Claude on every path**, as part of your conversation: your balance, next charge
date, autopay state, charges, payments, property address, your requests' subjects and
statuses, and the names and folders of your association's documents. Account numbers
are held back unless the model asks for `detailed`, which adds them, or two accounts
on one login share an address, when the number is the only way to tell them apart. The
document
links the portal issues need no sign-in, so no tool returns one, in any format. The
board directory is not served.

It reads the cached token from `$CPMFL_TOKEN`, then the keychain, on each call. A tool
never signs in, prompts, sends your password, or creates or changes a keychain item,
so a background server cannot lock your account by retrying a stale password, however
often it restarts. A missing or expired token is an error asking the user to run `cpmfl
auth login` in a terminal; while `$CPMFL_TOKEN` is set, the keychain is not read, so the
error asks the user to replace or unset the variable instead. The Claude Desktop
extension (`.mcpb`) takes no settings and so holds no credential of its own; on macOS
its bundled binary needs the user's approval, once, to read the `piekstra.cpmfl`
keychain item.

The config is `~/.config/cpmfl/config.json` and nothing else: no file in a project or
the working directory can redirect the token to another host.

Every tool's read goes through one seam that offers GETs alone.
`tests/mcp_readonly.rs` drives every tool through the real binary against a stub portal
and fails on anything but a GET to a listed read.

`cpmfl mcp --http --port <PORT>` serves the same tools over HTTP for the user's own
Cloudflare Worker, which reaches it through the user's own Cloudflare Tunnel
(cli-common's `connector/`). It binds 127.0.0.1 only, with no flag for another address,
and refuses to start without a bearer token of 32 or more characters, from
`CPMFL_MCP_TOKEN` or the keychain item `mcp-http-token` (`piekstra.cpmfl`), which `cpmfl
mcp --new-token` makes from 256 random bits and replaces only with `--overwrite`. Every
request must carry the token (compared in constant time) and a `Host` of `127.0.0.1` or
`localhost`, and must not carry an `Origin`; there are no CORS headers. Only the
read-only tools are served (all five are). The portal token stays on your computer: the
Worker sends tool calls and receives results, and never sees it.

**Tool results also cross Cloudflare on this path**, through your tunnel and your
Worker, on their way to Claude. Who holds what, and what a leaked token can do, is in
the connector's
[threat model](https://github.com/piekstra/cli-common/blob/main/connector/README.md#threat-model).

`cpmfl auth logout` removes the portal token and does not touch `mcp-http-token` or stop
`cpmfl mcp --http`. To revoke the connector's access, run `cpmfl mcp --new-token
--overwrite` and restart the server (`launchctl kickstart -k gui/$(id
-u)/com.piekstra.cpmfl-mcp-http`); the old token works until the restart. To stop
serving, run `launchctl bootout gui/$(id -u)/com.piekstra.cpmfl-mcp-http` and stop the
tunnel.

## Handling credentials safely

On Windows, whose Credential Manager holds 2,560 bytes per credential, a longer value is stored
across several credentials with the suffix `.pk-split-` and read back whole. After a downgrade,
run `cpmfl auth logout --forget` again from a current build: an older build's logout leaves the
parts.

Prefer piping from a password manager over typing:

```console
$ op read 'op://Private/your-portal-item/password' \
    | cpmfl auth set-credential --stdin --overwrite
```

`cpmfl auth logout --forget` removes both the password and the cached token
from the keychain and clears the config. It also removes the `mcp --http` token. It cannot
reach a running `cpmfl mcp --http`, which read its token at the start and keeps accepting
it until it stops, or a token in `$CPMFL_MCP_TOKEN`: stop the server (`launchctl bootout`
for its launchd agent), and unset the variable if it is set.
