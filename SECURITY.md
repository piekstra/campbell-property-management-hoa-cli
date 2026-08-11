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
- **Document URLs.** The portal returns direct links to document bytes;
  `cpmfl documents get` prints one only when explicitly asked. Never paste
  one into an issue or commit it.

## What this tool does not do

- It never mutates the portal — no payments, approvals, or profile changes.
- It talks only to the configured Vantaca service hosts and the document
  links those hosts return. No telemetry, no third-party services.
- It hardcodes no secrets. `gitleaks` runs in CI over the full history.

## Handling credentials safely

Prefer piping from a password manager over typing:

```console
$ op read 'op://Private/your-portal-item/password' \
    | cpmfl auth set-credential --stdin --overwrite
```

`cpmfl auth logout --forget` removes both the password and the cached token
from the keychain and clears the config.
