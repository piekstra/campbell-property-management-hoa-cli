# Fixtures

Real responses captured from the Campbell Property Management (Vantaca) portal
on **2026-08-06**, then scrubbed. `tests/fixture_shapes.rs` asserts that the
fields the commands read still exist, so a portal-side rename fails the build
instead of silently emptying a column.

## Scrubbing policy

This repo is written as if it were public. No fixture may carry anything that
identifies a real account, person, property, or balance.

Structure is preserved **exactly** — key names, nesting, types, null-vs-absent
— because that is what the tests assert on. Values are replaced:

| Value | Replaced with |
| --- | --- |
| Owner / board-member names | `Sample Owner` |
| Association name / code | `Sample Association, Inc` / `SAMPLE` |
| Management company | `Sample Property Management` |
| Email addresses | `owner@example.invalid` |
| Phone numbers | `555-0100` |
| Street address / city / state / ZIP | `100 Sample St`, `Sample City`, `ST`, `00000` |
| Account numbers | `SMP100000` |
| Record IDs (homeowner, owner, user, sub-ledger) | small fixed dummies |
| GUIDs | `00000000-0000-4000-8000-…` |
| Bank / card last four | `0000` |
| Money — dollar endpoints | `250` |
| Money — `/Ledger` (cents) | `25000` |
| Document blob URLs | `https://example.invalid/document-redacted` |
| Portal domain | `https://portal.example.invalid` |

Dates, booleans, enum-ish strings (`charge`, `payment`, `autoDraft`,
`chargeAmount`), folder names, statuses, and content types are kept verbatim —
they carry no personal information and the parsing logic depends on their
exact form.

**The 100× relationship between `/Ledger` and `/Charge` is deliberate.**
`250` dollars and `25000` cents describe the same amount, and
`ledger_reports_cents_while_charges_report_dollars` in `fixture_shapes.rs`
asserts it. Don't "fix" one of them to match the other.

Document *file names* are scrubbed too: they embed the association name in
free text even though `name` is otherwise a structural field.

## Refreshing

Capture with `cpmfl --json api --service <svc> <path> > raw.json`, then scrub
against the table above before committing. Never commit a raw capture — the
account data is real, and `documentUrl` values point at live association
documents.

`fixtures_carry_no_real_identifiers` in `tests/fixture_shapes.rs` enforces the
policy mechanically. Extend its banned list when you add a new kind of
identifier.
