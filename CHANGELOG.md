# Changelog

All notable user-facing changes are recorded here.

This project is still before its first stable release. Until then, entries may
describe release-candidate scope rather than long-term compatibility guarantees.

## 0.1.0 - 2026-05-27

- Initial MVP release candidate for `any-switch`.
- This is a macOS-evidenced stage release: macOS Claude OAuth import evidence is
  complete, while broader restart/risk experiments plus Linux and Windows
  manual evidence remain deferred follow-up work. Do not treat this release as
  full `docs/design.md` section 13 coverage.
- Built-in app definitions for Claude Code and OpenAI Codex.
- Definition-driven profile switching for file, JSON, TOML, environment, macOS
  Keychain, and OAuth capture state.
- User-defined app definitions and safe override exports for extending
  `any-switch` without app-specific core branches.
- Defensive backups, restore, drift detection, pending-switch recovery,
  target-lock serialization, process-safety checks, and redacted diagnostics.
- `doctor` reports current-platform OAuth capture source status, including
  credentials files and Keychain entries, without printing captured secret
  bytes.
- Codex file-backed ChatGPT OAuth, API-key, and legacy API-key import support,
  with unsupported credential stores rejected explicitly.
- Claude OAuth capture identity is taken from `oauthAccount`; opaque Claude
  credential tokens are captured as credentials, not decoded as identity.
- `--yes` is limited to high-risk writes and process-safety escape hatches:
  `add` and ordinary `import-current` reject it, while
  `import-current --yes` is valid only with `--assume-app-stopped`. Interactive
  terminals can type `yes` at the prompt instead of passing `--yes`.
  `--assume-app-stopped` is rejected when no matching running process was
  detected, so it cannot be used as a preemptive default flag.
- GitHub Actions release archives for Linux x86_64, macOS x86_64, macOS arm64,
  and Windows x86_64, staged before publish and verified with checksums.
- Release workflow uses Node 24-compatible actions, checks tag/Cargo version
  alignment, and publishes only after every target artifact is present.
- Release packaging uses temporary staging and temporary archive/checksum files
  before replacing final artifact names, making failed local packaging easier to
  retry safely.
- Optional macOS signing/notarization when GitHub release secrets are present;
  missing signing secrets do not block unsigned artifacts.
- Manual evidence helpers for Unix shells and Windows PowerShell generate
  redacted release-candidate evidence skeletons, including redaction for common
  JSON identity-name fields and Keychain account labels in diagnostic output.
- `doctor` capture-source warnings include a next-step hint, so Keychain or
  file-source diagnostics tell users how to verify metadata without revealing
  credential bytes.
