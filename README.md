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
cpmfl documents list --folder 38401       # drill into a folder
cpmfl documents search budget             # search the whole library
cpmfl documents download 1060 -o b.pdf    # fetch a document's bytes

cpmfl requests list --all                 # your service requests
cpmfl requests types                      # what this association accepts
cpmfl directory                           # board members and management
cpmfl calendar --since 2026-08-01         # community events
cpmfl reservations                        # amenity reservations you hold
cpmfl profile                             # your comms + directory-privacy settings
cpmfl association                         # enabled features and configuration

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
