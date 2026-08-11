# Contributing to campbell-property-management-hoa-cli

Thanks for your interest! Contributions — bug reports, fixes, new commands,
docs — are welcome under the project's dual [MIT](LICENSE-MIT) /
[Apache-2.0](LICENSE-APACHE) license: by opening a pull request you agree your
contribution is licensed under those same terms.

## Ground rules (this reads a real person's HOA portal)

- **Never commit secrets or personal data.** The portal password and the
  cached bearer token live only in the OS keychain. Nothing credential-like
  belongs in the repo, tests, fixtures, or commit messages.
- **Fixtures are scrubbed captures.** Structure is preserved exactly; every
  name, address, balance, amount, ID, and document URL is replaced with an
  obvious dummy. See [`tests/fixtures/README.md`](tests/fixtures/README.md) —
  and note that a test enforces it. Never commit a raw capture.
- **Read-only.** Every command observes. This CLI never mutates the portal —
  no payments, approvals, or profile changes — and pull requests should keep
  it that way.
- **Secrets never on argv or in logs.** Read them from the keychain or stdin.

## Dev loop

```console
$ make verify     # fmt-check + clippy -D warnings + tests + smoke — the CI gate
$ make test       # unit + integration (fully offline; no network, no creds)
$ make dev        # debug build, re-signed so keychain grants survive rebuilds
$ cargo run -- <command>
```

`make verify` is exactly what CI runs; a green local run predicts a green CI
run.

## Pull requests

1. Branch from `main`; keep the change focused.
2. Add or update tests. Contract tests load fixtures from disk — extend
   `tests/fixtures/` (scrubbed!) rather than inlining JSON blobs in test code.
3. Keep the human table and the `--json` DTO in sync. A breaking DTO change
   bumps its `schema` version suffix.
4. Update [`docs/api.md`](docs/api.md) when you learn something new about the
   portal's endpoints, and the README when you change the command surface.
5. Run `make verify` and include what you tested.

## A note on the portal

Vantaca publishes no homeowner-portal API, so everything here is reverse-
engineered and can break when they ship a redesign. If a command starts
returning empty columns, check `docs/api.md` against a fresh capture
(`cpmfl api <path>`) — a renamed field is the usual culprit, and the contract
tests are where the fix gets pinned down.

Testing a change against the live portal needs a real homeowner login. The
whole test suite is offline for exactly that reason; please keep it that way.
