<!--
Title: conventional-commits style, ≤ 70 chars, no trailing period.
Use feat(app): ... for App Features release notes.
Use feature(driver-<engine>): ... for Driver Features release notes.
e.g. feat(app): cancel an in-flight connection
e.g. feature(driver-postgres): introspect materialized views
Keep the PR focused — one logical change is easier to review and revert.
-->

## Summary

<!-- 1–3 bullets on the WHY: the problem this solves or the need it fills. -->

-

## Changes

<!-- What actually changed, grouped by area (app / core / driver-* / ci / docs). -->

-

## Test plan

<!-- Check what you ran; leave unchecked what still needs doing. -->

- [ ] `make fmt-check`
- [ ] `make lint`
- [ ] `make test` (or `make be-test` for backend-only changes)
- [ ] `make fe-build` / `make fe-run` for UI changes
- [ ] Manual verification steps (describe them):

## First pull request?

- [ ] I have signed the [Contributor License Agreement](../CLA.md) — the bot
      will comment below with the one-line reply that signs it.

## Notes for reviewers

<!-- Optional: screenshots for UI changes, trade-offs, follow-ups, anything risky. -->
