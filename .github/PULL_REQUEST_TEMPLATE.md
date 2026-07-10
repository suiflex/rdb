<!--
Title: conventional-commits style, ≤ 70 chars, no trailing period.
e.g. feat(app): cancel an in-flight connection
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

## Notes for reviewers

<!-- Optional: screenshots for UI changes, trade-offs, follow-ups, anything risky. -->
