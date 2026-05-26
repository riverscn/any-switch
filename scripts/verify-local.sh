#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

bash -n scripts/*.sh
if command -v pwsh >/dev/null 2>&1; then
  pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/manual-evidence.ps1 -Help >/dev/null
fi
git diff --check
cargo fmt --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
cargo package --locked --allow-dirty --offline

host_target="$(rustc -vV | sed -n 's/^host: //p')"
output_dir="$(mktemp -d "${TMPDIR:-/tmp}/any-switch-verify.XXXXXX")"
trap 'rm -rf "${output_dir}"' EXIT

bash scripts/package-release.sh "local-verify" "${host_target}" target/release/any-switch "${output_dir}"
(
  cd "${output_dir}"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "any-switch-local-verify-${host_target}.tar.gz.sha256"
  else
    sha256sum -c "any-switch-local-verify-${host_target}.tar.gz.sha256"
  fi
  tar -tzf "any-switch-local-verify-${host_target}.tar.gz" >/dev/null
)
