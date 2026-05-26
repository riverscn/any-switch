#!/usr/bin/env bash
set -euo pipefail

tag="${1:?usage: package-release.sh <tag> <target> [binary] [output-dir]}"
target="${2:?usage: package-release.sh <tag> <target> [binary] [output-dir]}"
binary="${3:-target/${target}/release/switch-cli}"
output_dir="${4:-.}"

if [[ ! "${tag}" =~ ^[A-Za-z0-9._-]+$ || ! "${target}" =~ ^[A-Za-z0-9._-]+$ ]]; then
  echo "tag and target may only contain letters, numbers, '.', '_', and '-'" >&2
  exit 2
fi

package="switch-cli-${tag}-${target}"
dist_dir="${output_dir}/dist"
package_dir="${dist_dir}/${package}"

if [[ -e "${package_dir}" ]]; then
  echo "package staging directory already exists: ${package_dir}" >&2
  exit 2
fi

mkdir -p "${package_dir}/docs" "${package_dir}/scripts" "${package_dir}/app_definitions/builtin"
cp "${binary}" "${package_dir}/switch-cli"
chmod 0755 "${package_dir}/switch-cli"
cp README.md CONTRIBUTING.md SECURITY.md LICENSE-APACHE LICENSE-MIT "${package_dir}/"
cp docs/design.md docs/release.md docs/acceptance.md docs/manual-verification.md docs/manual-evidence-template.md "${package_dir}/docs/"
cp scripts/manual-evidence.sh "${package_dir}/scripts/"
cp src/app_definitions/builtin/*.yaml "${package_dir}/app_definitions/builtin/"

tar -C "${dist_dir}" -czf "${output_dir}/${package}.tar.gz" "${package}"
(
  cd "${output_dir}"
  shasum -a 256 "${package}.tar.gz" > "${package}.tar.gz.sha256"
)
