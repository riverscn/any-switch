# Release Process

This project ships source plus prebuilt command-line binaries through GitHub
Actions.

Current release targets are Linux x86_64, macOS x86_64, macOS arm64, and
Windows x86_64.

## Local Verification

Run the same checks used by CI:

```bash
scripts/verify-local.sh
```

The repository pins the Rust toolchain in `rust-toolchain.toml`; run
`rustup toolchain install 1.95.0 --profile minimal --component rustfmt --component clippy`
before release verification if the pinned toolchain is not already installed.
The script also checks shell script syntax and patch whitespace, verifies the
Cargo source package offline, creates a temporary local archive through
`scripts/package-release.sh`, verifies its checksum, and lists the tarball. The
package script stages archive contents in a hidden temporary directory and
writes the tarball/checksum to temporary files before replacing the final
artifact names, so failed local packaging does not leave a reusable staging
tree or a half-written final archive.
If `pwsh` is installed locally, `scripts/verify-local.sh` also checks the
PowerShell manual-evidence helper's help path. Windows CI always runs the
PowerShell helper through Windows PowerShell.

`scripts/verify-local.sh` is the Unix/macOS local gate. Windows release behavior
is verified in GitHub Actions by the `windows build` job and the Windows branch
of the release matrix, including target-specific Clippy, selected Windows
smoke tests, `scripts/manual-evidence.ps1 -Help`, and packaging
`any-switch.exe` into a release archive.

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
The release workflow checks this before building artifacts.

## GitHub Actions Output

`.github/workflows/release.yml` first runs the release verification gate:

- `rustup toolchain install 1.95.0 --profile minimal --component rustfmt --component clippy`
- tag equals `v<Cargo.toml package version>`
- `scripts/verify-local.sh`

If verification passes, it builds:

- `x86_64-unknown-linux-gnu` on `ubuntu-latest`
- `x86_64-apple-darwin` on `macos-15-intel`
- `aarch64-apple-darwin` on `macos-15`
- `x86_64-pc-windows-msvc` on `windows-latest`

The Windows release branch also runs target-specific Clippy and smoke tests
before packaging:

- `cargo clippy --locked --target x86_64-pc-windows-msvc --all-targets -- -D warnings`
- file-lock contention
- Windows username detection
- Windows process-name matching and CSV parsing
- `scripts/manual-evidence.ps1 -Help`

The macOS runner labels are pinned by architecture instead of using
`macos-latest`, so release packages are built on the matching GitHub-hosted
runner family. As of May 27, 2026, GitHub's hosted runner reference documents
`macos-15-intel` as an Intel macOS label and `macos-15` as an arm64 macOS
label; re-check the reference before changing these labels.

The workflows intentionally use Node 24-compatible action lines:

- `actions/checkout@v6`
- `actions/upload-artifact@v7`
- `actions/download-artifact@v7`
- `softprops/action-gh-release@v3`

As of May 27, 2026, these are the current Node 24 lines documented by the
upstream action projects. Re-check the upstream action READMEs before changing
major versions. These versions are intended for GitHub-hosted runners on
GitHub.com. If a fork runs the workflows on self-hosted runners, keep the runner
application current enough for Node 24 actions before reusing this
configuration: `actions/checkout@v6` documents Actions Runner `v2.329.0` or
later for some authenticated Git paths, and the artifact actions document
Actions Runner `v2.327.1` or later for the Node 24 line. GitHub Enterprise
Server may require different artifact action versions; do not assume the
GitHub.com workflow can be copied unchanged to GHES.

## Optional macOS Signing

macOS binaries are signed before packaging only when signing secrets are present
in GitHub Actions. Missing secrets do not block release artifacts; the workflow
prints a skip message and packages the unsigned binary.

Required secrets for signing:

- `APPLE_DEVELOPER_ID_CERTIFICATE_BASE64`: base64-encoded Developer ID
  Application `.p12` certificate.
- `APPLE_DEVELOPER_ID_CERTIFICATE_PASSWORD`: password for the `.p12` file.
- `APPLE_CODESIGN_IDENTITY`: Developer ID Application signing identity name.

Additional secrets for notarization:

- `APPLE_ID`: Apple ID used with `notarytool`.
- `APPLE_APP_SPECIFIC_PASSWORD`: app-specific password for that Apple ID.
- `APPLE_TEAM_ID`: Apple Developer Team ID.

If signing secrets are present but notarization secrets are missing, the workflow
signs and verifies the Mach-O binary, skips notarization, and continues. If all
secrets are present, `scripts/sign-macos-binary.sh` signs the binary and submits
a temporary ZIP to Apple's notary service before packaging the signed binary in
the release tarball.

The build matrix packages each binary through `scripts/package-release.sh` as:

```text
any-switch-<tag>-<target>.tar.gz
```

`scripts/package-release.sh` removes its temporary staging directory after each
run. If packaging fails before the final move, only hidden temporary files are
eligible for cleanup by the script trap; callers should treat the named
`any-switch-<tag>-<target>.tar.gz` and `.sha256` files as the only release
outputs.

Each matrix job uploads its archive and checksum as a short-lived workflow
artifact. After all targets finish successfully, the `publish` job downloads all
workflow artifacts, verifies that every expected target has both a tarball and a
checksum, and then uploads the complete set to the GitHub Release. This avoids
leaving a partial GitHub Release when one target fails after another target has
already built.

The GitHub Release body is loaded from `CHANGELOG.md`, not generated solely from
commit metadata. This keeps the public release page aligned with the current
manual-evidence scope and prevents a stage release from omitting the
macOS-evidenced / deferred-evidence warning.

Because the workflow uses explicit `GITHUB_TOKEN` permissions, the build job
grants `artifact-metadata: write` for artifact upload, and the publish job
grants `actions: read` plus `artifact-metadata: read` for artifact download.
The publish job is the only job with `contents: write`.

Each non-Windows archive contains:

- `any-switch`

The Windows archive contains:

- `any-switch.exe`

Every archive also contains:

- `README.md`
- `CHANGELOG.md`
- `CODE_OF_CONDUCT.md`
- `CONTRIBUTING.md`
- `SECURITY.md`
- `LICENSE`
- `docs/user-guide.md`
- `docs/design.md`
- `docs/release.md`
- `docs/acceptance.md`
- `docs/evidence-followups.md`
- `docs/manual-verification.md`
- `docs/manual-evidence-template.md`
- `scripts/manual-evidence.sh`
- `scripts/manual-evidence.ps1`
- `app_definitions/builtin/*.yaml`

The GitHub Release also contains `any-switch-<tag>-<target>.tar.gz.sha256` next
to each archive. The release action uses `CHANGELOG.md` as the public release
body, and the same manually curated change log is included in each archive.

## Post-Release Checks

Download each archive from the release page and verify:

```bash
if command -v shasum >/dev/null 2>&1; then
  shasum -a 256 -c any-switch-<tag>-<target>.tar.gz.sha256
else
  sha256sum -c any-switch-<tag>-<target>.tar.gz.sha256
fi
tar -tzf any-switch-<tag>-<target>.tar.gz
tar -xzf any-switch-<tag>-<target>.tar.gz
./any-switch-<tag>-<target>/any-switch --version
```

For Windows archives, run the following in PowerShell:

```powershell
$archive = ".\any-switch-<tag>-x86_64-pc-windows-msvc.tar.gz"
$expected = ((Get-Content "${archive}.sha256") -split "\s+")[0].ToLowerInvariant()
$actual = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -ne $expected) { throw "checksum mismatch" }
tar -xzf .\any-switch-<tag>-x86_64-pc-windows-msvc.tar.gz
.\any-switch-<tag>-x86_64-pc-windows-msvc\any-switch.exe --version
powershell -NoProfile -ExecutionPolicy Bypass -File `
  .\any-switch-<tag>-x86_64-pc-windows-msvc\scripts\manual-evidence.ps1
```

For the current stage, run the macOS Claude OAuth import release blocker in
`docs/manual-verification.md` before tagging. Before claiming full MVP coverage,
also complete the deferred follow-up checks. The packaged evidence helpers can
initialize a redacted evidence file against the packaged binary.

On Linux and macOS:

```bash
ANY_SWITCH_BIN=./any-switch-<tag>-<target>/any-switch \
  ./any-switch-<tag>-<target>/scripts/manual-evidence.sh \
  manual-evidence-$(date -u +%Y%m%dT%H%M%SZ).md
```

On Windows, the PowerShell helper automatically uses the sibling
`any-switch.exe` from the extracted release directory:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File `
  .\any-switch-<tag>-x86_64-pc-windows-msvc\scripts\manual-evidence.ps1 `
  manual-evidence-<tag>-windows.md
```
