## Summary

Describe the change and the user-facing behavior it affects.

## Verification

- [ ] `scripts/verify-local.sh`
- [ ] Additional targeted tests or manual checks:

## Safety

- [ ] No secret values, OAuth capture blobs, API keys, Keychain contents, or real credential files are committed.
- [ ] Human output, JSON output, errors, and docs do not expose secret values.
- [ ] App-specific behavior stays in app definitions unless a trusted core handler is required.
- [ ] OAuth/process-sensitive flows preserve the stop-app requirement and documented escape hatch.

## Release Impact

- [ ] No release packaging impact.
- [ ] Release docs, packaging scripts, workflows, and `tests/release.rs` were updated if release artifacts changed.
- [ ] Manual evidence requirements were updated if this changes real-app behavior.
- [ ] Manual verification in `docs/manual-verification.md`, `docs/manual-evidence-template.md`, and `docs/evidence-followups.md` was considered before claiming stage-release or full MVP coverage.
