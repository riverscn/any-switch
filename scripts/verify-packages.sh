#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

version="$(cargo pkgid | sed -n 's/.*[#@]//p')"
if [[ -z "${version}" ]]; then
  echo "could not read package version from cargo pkgid" >&2
  exit 1
fi

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/any-switch-package-verify.XXXXXX")"
state_root="$(mktemp -d "${HOME}/.any-switch-package-verify.XXXXXX")"
trap 'rm -rf "${tmp_root}" "${state_root}"' EXIT

require_line() {
  local needle="$1"
  local file="$2"
  if ! grep -Fxq "${needle}" "${file}"; then
    echo "expected package listing to contain: ${needle}" >&2
    exit 1
  fi
}

reject_pattern() {
  local pattern="$1"
  local file="$2"
  if grep -Eq "${pattern}" "${file}"; then
    echo "package listing contains forbidden pattern: ${pattern}" >&2
    grep -En "${pattern}" "${file}" >&2 || true
    exit 1
  fi
}

echo "==> verifying Cargo source package"
cargo_package_list="${tmp_root}/cargo-package-list.txt"
cargo package --locked --allow-dirty --list >"${cargo_package_list}"
require_line "Cargo.toml" "${cargo_package_list}"
require_line "Cargo.lock" "${cargo_package_list}"
require_line "src/main.rs" "${cargo_package_list}"
require_line "README.md" "${cargo_package_list}"
require_line "README.zh-CN.md" "${cargo_package_list}"
reject_pattern '(^|/)(\.any-switch|\.claude|\.codex|\.smoke-|\.tmp)(/|$)' "${cargo_package_list}"
reject_pattern '^manual-evidence-[^/]*\.md$' "${cargo_package_list}"
reject_pattern '(^|/)(auth|credentials?)\.json$' "${cargo_package_list}"
cargo package --locked --allow-dirty --offline

echo "==> verifying Cargo install"
cargo_prefix="${tmp_root}/cargo-prefix"
cargo install --path . --locked --root "${cargo_prefix}"
"${cargo_prefix}/bin/any-switch" --version | grep -Fx "any-switch ${version}"
ANY_SWITCH_HOME="${state_root}/cargo-home" "${cargo_prefix}/bin/any-switch" apps | grep -Eq '(^|[[:space:]])claude([[:space:]]|$)'
ANY_SWITCH_HOME="${state_root}/cargo-home" "${cargo_prefix}/bin/any-switch" apps | grep -Eq '(^|[[:space:]])codex([[:space:]]|$)'

echo "==> verifying npm package"
npm_cache="${npm_config_cache:-${tmp_root}/npm-cache}"
npm_dry_run_json="${tmp_root}/npm-pack-dry-run.json"
npm_config_cache="${npm_cache}" npm pack --dry-run --json >"${npm_dry_run_json}"
grep -F '"path": "README.md"' "${npm_dry_run_json}" >/dev/null
grep -F '"path": "README.zh-CN.md"' "${npm_dry_run_json}" >/dev/null
if grep -F '"path": "docs/' "${npm_dry_run_json}" >/dev/null; then
  echo "npm package should not include docs/ files" >&2
  grep -F '"path": "docs/' "${npm_dry_run_json}" >&2
  exit 1
fi

npm_pack_dir="${tmp_root}/npm-pack"
mkdir -p "${npm_pack_dir}"
npm_tarball="$(npm_config_cache="${npm_cache}" npm pack --pack-destination "${npm_pack_dir}" --silent)"
npm_tarball_path="${npm_pack_dir}/${npm_tarball}"
npm_package_list="${tmp_root}/npm-package-list.txt"
tar -tzf "${npm_tarball_path}" >"${npm_package_list}"
require_line "package/README.md" "${npm_package_list}"
require_line "package/README.zh-CN.md" "${npm_package_list}"
require_line "package/package.json" "${npm_package_list}"
require_line "package/Cargo.toml" "${npm_package_list}"
require_line "package/src/main.rs" "${npm_package_list}"
reject_pattern '^package/docs/' "${npm_package_list}"
reject_pattern '(^|/)(\.any-switch|\.claude|\.codex|\.smoke-|\.tmp)(/|$)' "${npm_package_list}"
reject_pattern '^package/manual-evidence-[^/]*\.md$' "${npm_package_list}"
reject_pattern '(^|/)(auth|credentials?)\.json$' "${npm_package_list}"

echo "==> verifying npm install from packed tarball"
npm_prefix="${tmp_root}/npm-prefix"
npm_config_cache="${npm_cache}" npm install -g --prefix "${npm_prefix}" "${npm_tarball_path}"
"${npm_prefix}/bin/any-switch" --version | grep -Fx "any-switch ${version}"
ANY_SWITCH_HOME="${state_root}/npm-home" "${npm_prefix}/bin/any-switch" apps | grep -Eq '(^|[[:space:]])claude([[:space:]]|$)'
ANY_SWITCH_HOME="${state_root}/npm-home" "${npm_prefix}/bin/any-switch" apps | grep -Eq '(^|[[:space:]])codex([[:space:]]|$)'
