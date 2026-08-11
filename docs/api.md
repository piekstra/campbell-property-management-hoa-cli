# The Campbell Property Management portal API

`portal.campbellproperty.com` is a **Vantaca** homeowner portal. Vantaca
publishes no homeowner API; everything below was mapped by watching the
portal's own XHR traffic and reading its JavaScript bundles
(`/assets/index-*.js`, `elements-*.js`, `grids-*.js`), then confirmed against
a live account.

Nothing here is a stable contract. It can change without notice, and this
document is the record of what was true when it was written.

## Hosts

Vantaca fans the portal out over separate services. A path means nothing
without knowing which service serves it.

| Service | Base URL | Serves |
|---|---|---|
| users | `https://vantaca-api-users-prod-001.vantaca.net/api` | authentication, profile configuration |
| associations | `https://vantaca-api-associations-prod-001.vantaca.net/api` | documents, board directory, community calendar, feed |
| actionitems | `https://vantaca-api-actionitems-prod-001.vantaca.net/api` | homeowner requests, amenities, forms, work orders |
| pay | `https://api-pay-platform.vantaca.net/api` | account, ledger, payments, charges, autopay, payment methods |

Two further payment hosts exist for flows this CLI doesn't implement:
`api-pay-expresspay.vantaca.net` (pay without logging in) and
`api-pay-guestpay.vantaca.net` (pay on someone else's behalf). Each issues its
own token (`epayToken`, `guestPaymentsToken`) rather than reusing the session
JWT.

Separately, the portal's **"Pay Now"** button hands off to Western Alliance
Bank at `pay.westernalliancebank.com`, which is a different product with a
different login. It is out of scope here.

## Authentication

```http
POST {users}/Users/Authenticate
Origin: https://portal.campbellproperty.com
Content-Type: application/json

{"userName": "you@example.com", "password": "…"}
```

```jsonc
{
  "userId": 100000, "server": "..", "customer": "........",
  "serviceRegion": "....", "userName": "Jane Homeowner",
  "userTypeId": "1", "lastLoginDate": "2026-01-01T00:00:00.000Z",
  "token": "<JWT>", "otpId": null,
  "shouldResetPassword": null, "isValid": true
}
```

Every later request sends `Authorization: Bearer <token>`. There is no cookie
session, no CSRF token, and **no two-factor step** on the password path.

### `Origin` is load-bearing

Vantaca is multi-tenant and the API hosts are shared across every management
company, so the *only* thing identifying which company a login belongs to is
the `Origin` header. Omit it and a perfectly valid password fails with:

```json
{"message": "Invalid company"}
```

That is why `cpmfl` sends `Origin` and `Referer` on every request, and why
`portal_url` is a config key — pointing it at another Vantaca-hosted company
is all it takes to talk to a different tenant.

### Token expiry

The JWT carries standard `exp` / `iat` claims plus Vantaca's own `Server`,
`Customer`, `Client`, `UserId`, `UserTypeId`. `cpmfl` decodes `exp` locally so
an expired session reports as exit 3 with a "run `cpmfl auth login`" message
rather than a mystery 401. The server remains the authority — a token that
can't be decoded is treated as live and sent anyway.

### The one-time-code path (not implemented)

The portal also offers a passwordless login: `POST {users}/OneTimePassword`
with `{"email": …, "shouldResetPassword": false}` emails a code (204 No
Content), then `POST {users}/OneTimePassword/verify` exchanges it for a token.
`cpmfl` uses the password path instead, because a code requires reading the
inbox mid-command. Worth revisiting if password login is ever restricted.

`POST {users}/Users/Authenticate/PortalToken` exchanges the JWT for a 36-char
GUID used by the embedded payment UI. Not needed for reads.

## Conventions

**Paged envelope.** Most list endpoints return:

```json
{"page": 1, "lastPage": 3, "totalItems": 21, "member": [ … ]}
```

Controlled by `page` and `pageSize` (default 10). Document *search* is the
exception — see below.

**Unknown query parameters are silently ignored.** This is the sharpest edge
in the whole API. `GET /Ledger?startDate=2026-07-01` returns **200 with the
complete unfiltered set**. Nothing signals that the filter was dropped, so a
naive client reports a filtered view that isn't one. Consequences:

- `cpmfl` filters `--since` / `--until` **client-side**, always.
- Directory navigation is `Directories?parentId=<id>`. Spelling it
  `folderId`, `directoryId`, or `id` returns the *root* listing with a 200.

**Money scale is inconsistent.** `/Ledger` reports integer **cents**
(`25000`); `/Payment`, `/Charge`, and `/account` report **dollars** (`250`)
for the very same transaction. Getting this wrong inflates a ledger 100×.
`cpmfl` keeps the scale at each call site (`money` vs `money_cents`).

**Dates** are ISO-8601 datetimes (`2026-08-01T00:00:00`). The CLI truncates to
`YYYY-MM-DD` at its boundary.

**Errors** use a consistent envelope; `message` and `detail` are the
human-readable fields:

```json
{"title": "Forbidden", "status": 403, "detail": "Reservation requests are not
 enabled for this association.", "errorId": "…", "errorCode": "CM003",
 "trace": null, "message": "…"}
```

`403` is usually a *capability* answer ("this association hasn't enabled
that"), not an expired session, so `cpmfl` maps it to exit 4 rather than
sending the user into a pointless re-login.

Association-scoped board endpoints (`/associations/{a}/homeowners/{ho}/…`)
answer **500** rather than 403 for a non-board login. Treat a 500 there as
"not permitted", not as an outage.

## Reads this CLI uses

| Command | Service | Endpoint |
|---|---|---|
| `summary`, `balance`, `account` | pay | `GET /account` |
| `properties` | pay | `GET /PropertyOwners` |
| `transactions` | pay | `GET /Ledger?page&pageSize` |
| `payments` | pay | `GET /Payment?page&pageSize` |
| `charges` | pay | `GET /Charge` |
| `autopay` | pay | `GET /AutoDraft` |
| `scheduled` | pay | `GET /RecurringPayment`, `GET /FuturePayment` |
| `payment-methods` | pay | `GET /PaymentMethod` |
| `association` | pay + associations | `GET /Features`, `GET /associations/{a}/features`, `GET /associations/{a}/community-feed/config` |
| `documents list` | associations | `GET /associations/{a}/Directories[?parentId=]` |
| `documents search` | associations | `GET /associations/{a}/Documents/Search?search=&page=` |
| `documents download` | associations | `GET /associations/{a}/Documents/{id}` (streams the bytes) |
| `directory` | associations | `GET /associations/{a}/directory` |
| `calendar` | associations | `GET /associations/{a}/community-calendar` |
| `requests list` | actionitems | `GET /HomeownerRequests?page&pageSize` |
| `requests types` | actionitems | `GET /Home/GeneralRequests/RequestTypes` |
| `reservations` | actionitems | `GET /Amenities/MyReservations` |

Document *search* answers with its own envelope — `{page, pageSize,
totalResults, totalPages, documents[]}` — not the `member`/`totalItems` one.
The `search` parameter is required and must be non-empty.

The `documentUrl` each file row carries is a **short-lived Azure blob SAS
URL** built from the document's file name verbatim, spaces and all — not a
legal request-target as issued. Worse, the path already carries `%xx` escapes,
so re-encoding the whole URL double-encodes them and breaks the SAS signature
(403). `cpmfl` therefore emits these URLs with only the invalid bytes
percent-encoded, and `documents download` avoids the SAS URL entirely by
streaming `Documents/{id}` through the portal.

### Reads that exist but aren't wired up

`GET {associations}/associations/{a}/directory/types`,
`/event-categories`, `/Directories` breadcrumbs, and
`GET {users}/Users/profile/configuration` are all read and unused. Board-role
reads (`/Board/*`, `/board/requests/*`) exist throughout but need a board
login to return anything.

## Write and write-esque capabilities — NOT implemented

`cpmfl` is read-only. Of the **208** distinct paths recovered from the
bundles, **83** carry a non-GET method. They are catalogued here so adding one
later is a deliberate act, not a discovery.

Anything added from this list must, per `AGENTS.md`, prompt for confirmation
and require `--force` when non-interactive (exit 6 otherwise) — and the
"read-only" claim must come out of the docs.

### Moves money — highest blast radius

| Endpoint | Effect |
|---|---|
| `POST {pay}/Payment/Setup` → `PUT /Payment/Setup/{id}/Confirm` | Make a one-time payment. Two-step: setup then confirm. |
| `DELETE {pay}/Payment/Setup/{id}` | Cancel a payment mid-setup. |
| `POST {pay}/AutoDraft/Setup` → `PUT /AutoDraft/Setup/Confirm` | Enroll in autopay. |
| `PATCH`/`DELETE {pay}/AutoDraft/{id}` | Change or cancel autopay enrollment. |
| `POST {pay}/RecurringPayment/Setup` → `PUT /RecurringPayment/Setup/{id}/Confirm` | Create a recurring payment. |
| `PATCH`/`DELETE {pay}/RecurringPayment/{id}` | Change or cancel a recurring payment. |
| `POST {pay}/FuturePayment/Setup` → `PUT /FuturePayment/Setup/{id}/Confirm` | Schedule a future-dated payment. |
| `PATCH`/`DELETE {pay}/FuturePayment/{id}` | Change or cancel a scheduled payment. |
| `POST {pay}/PaymentMethod/Setup`, `POST {pay}/account/payment-methods/setup` | Add a bank account or card. |
| `DELETE {pay}/PaymentMethod/{id}` | Remove a saved payment method. |
| `POST {pay}/check-migration/one-time`, `/check-migration/recurring` | Migrate check payments to electronic. |
| `POST {pay}/express-pay/*`, `{pay}/payment/*`, `{pay}/recurring-payments/*` | The express/guest pay equivalents, on their own tokens. |

### Changes your identity or account

| Endpoint | Effect |
|---|---|
| `PUT {users}/Users/Password` | Change the account password. |
| `POST {users}/Homeowner/Registration`, `/Registration/{id}` | Register a homeowner account. |
| `PATCH {users}/Homeowner/{id}/Preferences` | Change communication preferences. |
| `POST`/`PUT`/`DELETE {users}/Homeowner/{id}/address[/{id}]` | Add, edit, or remove a mailing address. |
| `POST`/`PATCH`/`DELETE {users}/Homeowner/{id}/contact[/{id}]` | Add, edit, or remove a phone/email contact. |
| `PUT {pay}/account/reminder-text-enrollment` | Enroll in / cancel SMS billing reminders. |
| `POST`/`PATCH {users}/Users/{id}/OnboardingSession[s]` | Drive the onboarding wizard. |
| `PUT {users}/OneTimePassword/reset`, `POST /OneTimePassword` | Trigger a password-reset or login code email. Sends real mail. |

### Submits things to the association

| Endpoint | Effect |
|---|---|
| `POST {actionitems}/Home/GeneralRequests` | Submit a general request. |
| `POST {actionitems}/WorkOrders` | Submit a work order. |
| `POST {actionitems}/Home/ArcRequests` | Submit an architectural-review application. |
| `POST`/`DELETE {actionitems}/Home/ArcRequests/Drafts[/{id}][/Attachments[/{id}]]` | Manage ARC drafts and their attachments. |
| `POST {actionitems}/Home/FormSubmissions` | Submit an association form. |
| `POST`/`PATCH`/`DELETE {actionitems}/Amenities/ReservationRequest[/{id}]` | Book, change, or cancel an amenity reservation. |
| `PATCH {actionitems}/Messages/MarkRead` | Mark portal messages read. |
| `PUT {actionitems}/Acknowledgement/{id}/Confirm` | Acknowledge a notice. |

### Board-only — acts on behalf of the association

Available only to a board login, and consequential for other people. Listed
for completeness; a homeowner token gets 403/500.

| Endpoint | Effect |
|---|---|
| `POST {actionitems}/Board/BoardInvoices/{id}/Approve`, `/Decline`, `/BulkApprove` | Approve or decline association invoices. |
| `POST`/`PUT`/`DELETE {actionitems}/Board/BoardInvoices/{id}/GeneralLedgerDetails[/{id}]` | Edit invoice GL coding. |
| `POST {actionitems}/Board/ArcRequests/{id}/Vote`, `/ChairDecision` | Vote on a neighbour's ARC application. |
| `PUT {actionitems}/Board/BoardViolations/{id}/ChangeStep`, `/BoardWorkOrders/{id}/ChangeStep`, `/BoardCollections/{id}/ChangeStep` | Advance violation / work-order / collections workflows. |
| `PUT {actionitems}/Board/BoardInspections/{id}/ReviewComplete`, `/Violations/{id}/Note` | Complete inspections, annotate violations. |
| `POST {actionitems}/Board/BoardInspections/Violations/{id}/Messages`, `/Board/ArcRequests/Message` | Send messages to homeowners. |
| `POST`/`PUT {actionitems}/BoardCommunications`, `/ClearMessages` | Association-wide communications. |
| `POST`/`PATCH`/`DELETE {associations}/associations/{a}/Documents/{id}`, `/Directories/{id}` | Upload, rename, or delete library documents. |
| `POST`/`PATCH`/`DELETE {associations}/associations/{a}/community-calendar[/{id}]` | Manage community calendar events. |

## Reproducing this map

```console
curl -s https://portal.campbellproperty.com/public \
  | grep -oE '/assets/[^"]+\.js'
```

Fetch those bundles and extract call sites — they are minified but the paths
survive as literals and template strings:

```console
grep -ohE '\.(get|post|put|patch|delete)\(`[^`]{1,100}`' *.js | sort -u
```

Template placeholders appear as `${e}` / `${t}`; substituting `:id` gives the
route shape. Response shapes are best confirmed live: log into the portal in a
browser, then `fetch` an endpoint from the devtools console with the
`Authorization` header from `localStorage["user-store"].state.token`.
