# Security Policy

## Supported versions

RDBS is pre-1.0 and ships from a single tracked package (`rdbs`). Security
fixes land on the latest release; there are no long-term support branches yet.

| Version | Supported          |
| ------- | ------------------ |
| 0.8.x   | :white_check_mark: |
| < 0.8   | :x:                |

Please upgrade to the latest release before reporting an issue where possible.

## Reporting a vulnerability

**Do not report security vulnerabilities through public GitHub issues,
discussions, or pull requests.**

Instead, use GitHub's private vulnerability reporting:

1. Go to the [Security tab](https://github.com/suiflex/rdb/security/advisories/new).
2. Click **Report a vulnerability** and fill out the advisory form.

If you cannot use private reporting, contact the maintainers (**@suiflex**) and
ask for a private channel before sharing any details.

Please include, where you can:

- The affected component (`app`, `core`, `connstore`, or a specific
  `driver-*` crate) and version.
- A description of the vulnerability and its impact.
- Steps to reproduce, a proof of concept, or the relevant configuration.
- Any suggested remediation.

## What to expect

- **Acknowledgement** within 5 business days.
- An initial assessment and severity triage shortly after.
- Progress updates as we work on a fix, and coordination on a disclosure
  timeline. We aim to release a fix before any public disclosure.
- Credit for the reporter in the advisory, unless you prefer to remain
  anonymous.

## Scope

RDBS stores connection secrets in the OS keychain, falling back to AES-GCM
encryption. Reports touching credential handling, secret storage, or the
database driver connection paths are especially valued.

Thank you for helping keep RDBS and its users safe.
