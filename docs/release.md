# Release Process

This project ships source-build packages through Cargo and npm. It does not
publish unsigned prebuilt macOS or Windows binaries as end-user installation
artifacts.

## Local Verification

Run the same checks used by CI:

```bash
scripts/verify-local.sh
```

The repository pins the Rust toolchain in `rust-toolchain.toml`; run
`rustup toolchain install 1.95.0 --profile minimal --component rustfmt --component clippy`
before release verification if the pinned toolchain is not already installed.
The script also checks shell script syntax and patch whitespace, verifies the
Cargo source package offline, creates a temporary local runtime archive through
`scripts/package-release.sh`, verifies its checksum, and lists the tarball. This
archive path is retained for local smoke testing and future signed-binary work;
it is not uploaded as the public release artifact in the current source-build
distribution model.
If `pwsh` is installed locally, `scripts/verify-local.sh` also checks the
PowerShell manual-evidence helper's help path. Windows CI always runs the
PowerShell helper through Windows PowerShell.

`scripts/verify-local.sh` is the Unix/macOS local gate. Windows build behavior
is verified in GitHub Actions by the `windows build` job, including
target-specific Clippy, selected Windows smoke tests,
`scripts/manual-evidence.ps1 -Help`, and a local package smoke for
`any-switch.exe`. This proves the code builds on Windows without distributing an
unsigned Windows executable to users.

CI uses a per-ref concurrency group and cancels older in-progress runs for the
same branch or pull request. Release runs use a per-tag concurrency group but do
not cancel in-progress jobs, so a repeated tag push waits behind the active
release job instead of racing the GitHub Release publish step.

## Release-Candidate Evidence Gate

CI proves the repository builds and packages the supported targets, but it does
not prove real Claude Code / Codex behavior on every OS. For the current stage,
complete the macOS Claude OAuth import release blocker in
`docs/manual-verification.md` and record the result using
`docs/manual-evidence-template.md`. Broader macOS restart/risk experiments plus
Linux and Windows manual evidence may be deferred to follow-up release work, but
release notes must describe the build as a macOS-evidenced stage release and
must not claim full MVP / section 13 coverage until deferred evidence is
complete.

Keep generated `manual-evidence-*.md` files out of Git unless the project
explicitly decides to publish a fully redacted record. Attach final evidence to
a private issue, release checklist, or other maintainer-visible system. A
release candidate must not claim full `docs/design.md` section 13 coverage while
any deferred manual evidence item in `docs/acceptance.md` is unchecked or lacks
a linked follow-up issue. Use `docs/evidence-followups.md` as the in-repository
tracker until those external links exist. The repository issue form
`.github/ISSUE_TEMPLATE/release_checklist.yml` is the default public checklist
template for non-sensitive release evidence.

For the current macOS-evidenced stage release, copy only a short redacted
summary into the release checklist. Do not attach Keychain values, OAuth capture
files, full `~/.claude.json`, full `auth.json`, or unredacted identity fields.
A sufficient checklist entry is:

```text
Current-stage blocker:
- 2: macOS Claude OAuth import: passed.
  Evidence summary:
  - scripts/verify-local.sh passed on macOS arm64.
  - any-switch doctor claude reported no running Claude process before import.
  - any-switch import-current claude manual-macos --kind oauth_capture succeeded.
  - any-switch show claude-manual-macos showed oauth_capture and required identity fields, with values redacted.
  - any-switch status claude reported matched.

Deferred items:
- tracked in docs/evidence-followups.md.
```

## Tagging

Create and push a semantic version tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

The tag must match the package version in `Cargo.toml` with a leading `v`
prefix. For example, `version = "0.1.0"` must be released from tag `v0.1.0`.
The release workflow checks this before publishing release notes.

## GitHub Actions Output

`.github/workflows/release.yml` first runs the release verification gate:

- `rustup toolchain install 1.95.0 --profile minimal --component rustfmt --component clippy`
- tag equals `v<Cargo.toml package version>`
- `scripts/verify-local.sh`

If verification passes, the release workflow publishes GitHub Release notes from
`CHANGELOG.md`. It does not upload prebuilt binary assets while macOS and
Windows signing/notarization are not part of the public distribution path.

The workflows intentionally use Node 24-compatible action lines:

- `actions/checkout@v6`
- `softprops/action-gh-release@v3`

As of May 27, 2026, these are the current Node 24 lines documented by the
upstream action projects. Re-check the upstream action READMEs before changing
major versions. These versions are intended for GitHub-hosted runners on
GitHub.com. If a fork runs the workflows on self-hosted runners, keep the runner
application current enough for Node 24 actions before reusing this
configuration: `actions/checkout@v6` documents Actions Runner `v2.329.0` or
later for some authenticated Git paths. GitHub Enterprise Server may require
different action versions; do not assume the GitHub.com workflow can be copied
unchanged to GHES.

## Binary Signing

Do not publish prebuilt macOS or Windows binaries until signing and
notarization are part of the release policy. `scripts/sign-macos-binary.sh`
remains in the repository for future signed-binary work, but the current release
workflow does not upload unsigned binaries as public assets.

The GitHub Release body is loaded from `CHANGELOG.md`, not generated solely from
commit metadata. This keeps the public release page aligned with the current
manual-evidence scope and prevents a stage release from omitting the
macOS-evidenced / deferred-evidence warning.

Because the workflow uses explicit `GITHUB_TOKEN` permissions, the verify job
runs with read-only repository contents permission. The publish job is the only
job with `contents: write`, and it uses that permission only to create or update
the GitHub Release notes.

Built-in app definitions are compiled into the binary at build time, so release
source-build packages do not need to install `app_definitions/builtin/*.yaml`
next to the executable. The source repository and Cargo source package remain
the auditable record for those definitions.

## npm Package

The npm package bundles the Rust source files needed to build `any-switch`
locally. During `npm install`, `npm/install.js` checks for `cargo`, runs
`cargo build --release --locked`, copies the resulting binary into `vendor/`,
and exposes it through the `any-switch` npm bin shim. npm does not install
Rust itself; users must install Rust first.

Before publishing to npm:

```bash
npm_config_cache=/private/tmp/any-switch-npm-cache npm pack --dry-run
packdir=$(mktemp -d /private/tmp/any-switch-npm-pack.XXXXXX)
package_tarball=$(npm_config_cache=/private/tmp/any-switch-npm-cache npm pack --pack-destination "$packdir" --silent)
tmpdir=$(mktemp -d /private/tmp/any-switch-npm-prefix.XXXXXX)
npm_config_cache=/private/tmp/any-switch-npm-cache npm install -g --prefix "$tmpdir" "$packdir/$package_tarball"
"$tmpdir/bin/any-switch" --version
```

## Cargo Package

Cargo is the primary Rust-native distribution path:

```bash
cargo publish --dry-run --locked
cargo publish --locked
```

Users can then install with:

```bash
cargo install any-switch --locked
```

## Post-Release Checks

Verify the published package paths:

```bash
npm view any-switch version
cargo search any-switch
```

Install into temporary locations where possible:

```bash
tmpdir=$(mktemp -d /private/tmp/any-switch-npm-post.XXXXXX)
npm_config_cache=/private/tmp/any-switch-npm-cache npm install -g --prefix "$tmpdir" any-switch
"$tmpdir/bin/any-switch" --version

cargo install any-switch --locked --root /private/tmp/any-switch-cargo-post
/private/tmp/any-switch-cargo-post/bin/any-switch --version
```

For the current stage, run the macOS Claude OAuth import release blocker in
`docs/manual-verification.md` before tagging. Before claiming full MVP coverage,
also complete the deferred follow-up checks. The manual evidence helpers remain
in the source repository; use `ANY_SWITCH_BIN` to point them at the binary being
checked.

On Linux and macOS:

```bash
ANY_SWITCH_BIN="$(command -v any-switch)" \
  scripts/manual-evidence.sh \
  manual-evidence-$(date -u +%Y%m%dT%H%M%SZ).md
```

On Windows:

```powershell
$env:ANY_SWITCH_BIN = (Get-Command any-switch).Source
powershell -NoProfile -ExecutionPolicy Bypass -File `
  scripts\manual-evidence.ps1 `
  manual-evidence-<tag>-windows.md
```
