# cpmfl

The **Campbell Property Management** HOA homeowner portal
([portal.campbellproperty.com](https://portal.campbellproperty.com)) from the
command line — balance, ledger, payments, documents, and requests.

Unofficial and unaffiliated. The portal is a [Vantaca](https://vantaca.com)
tenant and publishes no homeowner API; this talks to the same undocumented
JSON endpoints the portal's own front end calls, mapped in
[`docs/api.md`](docs/api.md).

**Read-only.** Every command observes; none mutate. The portal can move money,
enroll you in autopay, submit requests, and book amenities — none of that is
implemented, and `docs/api.md` catalogs those endpoints so adding one later is
a deliberate act.

Part of the [`piekstra-cli/1`](https://github.com/piekstra/cli-common) family:
same `--json` contract, same exit codes, same `auth`/`config`/`self-update`
commands as its siblings. It implements the `utility/v1` domain profile, so
drivers like [`utiman`](https://github.com/piekstra/utiman) read it with no
per-provider configuration.

## Install

```console
make install
```

On macOS this re-signs the installed binary with a stable identity so the
one-time keychain "Always Allow" grant survives future reinstalls; a bare
`cargo install --path .` ad-hoc signs and re-prompts each time. (See
`cli-common/scripts/setup-dev-signing.sh` for the one-time identity setup.)

## Getting started

```console
cpmfl config set username you@example.com
cpmfl auth login          # prompts for the portal password, caches a token
cpmfl summary
```

The password and the resulting bearer token live in the OS keychain (service
`piekstra.cpmfl`, accounts `password` and `token`) — never on argv, never in a
file. For headless setup, pipe it in:

```console
op read "op://Private/portal.campbellproperty.com/password" \
  | cpmfl auth set-credential --stdin
```

## Commands

```console
cpmfl summary                     # balance, next charge, autopay (utility-summary/v1)
cpmfl balance                     # the same DTO, by its other name
cpmfl account                     # association, owner, property, portal blocks
cpmfl properties                  # every property/ledger this login can see

cpmfl transactions --since 2026-01-01     # ledger with running balance
cpmfl payments --limit 10                 # posted and pending payments
cpmfl charges                             # assessed charges and due dates
cpmfl autopay                             # AutoDraft enrollment and schedule
cpmfl scheduled                           # recurring + future-dated payments
cpmfl payment-methods                     # saved methods (last four only)

cpmfl documents list                      # document library root
cpmfl documents list --folder 1050        # drill into a folder
cpmfl documents search budget             # search the whole library
cpmfl documents download 1060 -o b.pdf    # fetch a document's bytes

cpmfl requests list --all                 # your service requests
cpmfl requests types                      # what this association accepts
cpmfl directory                           # board members and management
cpmfl calendar --since 2026-08-01         # community events
cpmfl reservations                        # amenity reservations you hold
cpmfl profile                             # your comms + directory-privacy settings
cpmfl association                         # enabled features and configuration

cpmfl mcp                                 # read-only tools for Claude (see Use with Claude)

cpmfl api /Ledger --query pageSize=5      # raw passthrough (GET only)
```

Every command takes `--json`:

```console
$ cpmfl --json summary
{
  "schema": "utility-summary/v1",
  "balance": { "amount": "0.00", "currency": "USD" },
  "due_date": "2026-10-01",
  "account": "SMP100000",
  "autopay": true
}
```

```console
$ cpmfl --json transactions --limit 1
{
  "schema": "transaction-list/v1",
  "items": [
    {
      "date": "2026-07-04",
      "amount": { "amount": "-250.00", "currency": "USD" },
      "description": "ACH ...0000",
      "kind": "payment",
      "running_balance": { "amount": "0.00", "currency": "USD" },
      "voided": false
    }
  ],
  "total": 21
}
```

Exit codes: `0` ok · `2` usage · `3` auth · `4` not found · `5` upstream.
On failure with `--json`, stdout carries `{"error": {"code", "message"}}`.

## Use with Claude

`cpmfl mcp` serves five read-only tools to Claude over MCP: `campbell_balance`,
`campbell_bills`, `campbell_payments`, `campbell_requests` and `campbell_documents`.
Nothing can make a payment, change autopay, submit a request or change anything at the
portal. Document results name documents and carry no link and no contents. The board
directory is not served, since it names other people.

**Claude Code.** After `cpmfl auth login`:

```sh
claude mcp add campbell -- cpmfl mcp
```

Or install the `campbell-property-management-hoa-cli` plugin from
[claude-plugins](https://github.com/piekstra/claude-plugins), which runs the same
command.

**Claude Desktop.** Download `cpmfl-<version>.mcpb` from the
[latest release](https://github.com/piekstra/campbell-property-management-hoa-cli/releases/latest)
and open it. It asks for nothing: the bundled `cpmfl` reads the token your installed
`cpmfl` cached. On macOS the bundled binary is a different program to the keychain, so
its first call raises a keychain prompt for the `piekstra.cpmfl` item; choose **Always
Allow**, or every call will ask again.

Any other MCP client that launches a local server can run `cpmfl mcp`.

The tools never sign in and never send your password. When the portal's session
expires, they say so, and you run `cpmfl auth login` again. They read `$CPMFL_TOKEN`
first if it is set, then the keychain. A token in that variable that has expired must
be replaced or unset where it is set, and the server restarted, since the keychain is
not read while it is set.

**Claude on the web and mobile.** claude.ai reaches tools through a remote connector,
and your portal session stays on your computer, so `cpmfl` serves the same tools from
your computer to a connector you deploy into your own Cloudflare account:

```console
$ cpmfl mcp --new-token            # once: a bearer token, saved in the keychain and printed for the Worker
$ cpmfl mcp --http --port 8750     # serve on 127.0.0.1 only
```

The connector, [cli-common's `connector/`](https://github.com/piekstra/cli-common/tree/main/connector#tools-from-your-computer-home-modules),
reaches that port through a Cloudflare Tunnel in your account; its README walks through
the tunnel, the VPC service, a launchd agent that keeps the server running, and the
`HOME_CAMPBELL_TOKEN` secret, with the threat model. `--http` binds 127.0.0.1 only,
refuses to start without the token, refuses any request whose `Host` is not loopback or
that carries an `Origin`, and serves the read-only tools only. Your portal password and
session never leave your computer, but tool results do: your balance, charges,
payments, property address, requests and the association's document names pass through
Cloudflare and reach Claude ([`SECURITY.md`](SECURITY.md)). While the computer sleeps,
the tools answer that it is offline.

After `cpmfl self-update`, restart the server: a running one keeps the old code, and its
launchd agent restarts it only when it exits. Its start line names the version.

```console
$ launchctl kickstart -k gui/$(id -u)/com.piekstra.cpmfl-mcp-http
```

`self-update` does not reach a Claude Desktop extension, which runs its own copy.
Install the new release's `.mcpb` too.

## Notes from the portal

Three things about this API are worth knowing before trusting output:

- **Unknown query parameters are silently ignored.** `GET /Ledger?startDate=…`
  returns 200 with the *complete unfiltered* set. That is why `--since` and
  `--until` are applied client-side here, always.
- **Money scale is inconsistent.** `/Ledger` reports integer cents; `/Payment`,
  `/Charge`, and `/account` report dollars — for the same transaction. Getting
  it wrong inflates a ledger 100×.
- **`Origin` identifies the management company.** Vantaca's API hosts are
  shared across tenants, so a login without the right `Origin` header fails
  with "Invalid company" even when the password is correct. Point `portal_url`
  at another Vantaca-hosted company and the rest of the CLI follows.

`payment-methods` can be empty while `autopay` clearly drafts from a bank
account: the portal keeps an autopay funding source separate from the saved
methods used for one-off payments. `autopay` reports its own
`bank_account_last4`.

Payments are a separate product: the portal's "Pay Now" button hands off to
Western Alliance Bank, which has its own login and is out of scope here.

## Development

```console
make verify    # fmt-check + clippy -D warnings + tests + smoke — the CI gate
make test      # fully offline; no network, no credentials
make dev       # debug build, re-signed so keychain grants survive rebuilds
```

Tests run against scrubbed captures in
[`tests/fixtures/`](tests/fixtures/README.md); read that README before
touching one.

## License

MIT OR Apache-2.0, at your option.
