# CLAUDE.md

The canonical agent guide for this repo is **[AGENTS.md](AGENTS.md)** — read it
first. It covers build/test/lint, layout, conventions, and the safety rules.

Claude Code specifics:

- **Gate on `make verify`.** Don't report a change as done until it's green
  (fmt + clippy `-D warnings` + tests + smoke). Tests are fully offline.
- **This CLI is read-only.** Don't add a command that writes to the portal
  without being asked; payments, autopay enrollment, request submission, and
  amenity booking are deliberately out of scope. `docs/api.md` lists the
  endpoints they would use.
- **Secrets:** the portal password and bearer token live in the OS keychain
  (`piekstra.cpmfl`). Never print one, put it on argv, or write it to a file —
  including while debugging. Beware `2>&1 >/dev/null` when running `op`: the
  redirection order sends the secret to the visible stream, not to `/dev/null`.
- **Testing against the live portal is cheap but not free.** Logging in is a
  single request with no two-factor step, so a smoke test costs little — but
  every read hits a real homeowner account. Prefer the offline fixtures.
- **Two numbers that look wrong usually aren't.** `/Ledger` is in cents while
  `/Charge` is in dollars, and a filter parameter the portal ignores still
  returns 200. Check `AGENTS.md` § Portal-specific gotchas before "fixing"
  either.
- **"Deployed" means released + installed.** A change isn't live until a
  release is cut (tag `v*` → the release workflow) and the binary is installed
  or `self-update`d on the target machine.
- **Written as if public.** No secrets, real names, addresses, balances,
  account numbers, or association names in any diff — including test fixtures
  (dummies only).
