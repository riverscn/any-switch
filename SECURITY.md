# Security Policy

`any-switch` manages local credentials and application state. Please report
security issues privately instead of opening a public issue.

## Supported Versions

Until the first stable release, security fixes target the main branch.

## Reporting a Vulnerability

Send a report to the project maintainers through the repository security
advisory flow, or contact the maintainer listed for the repository. Include:

- affected version or commit;
- operating system;
- exact command or workflow involved;
- whether secret values, capture blobs, backup files, or target application
  files can be exposed or corrupted;
- a minimal reproduction when possible.

## Security Boundaries

- The tool does not perform login or account recovery.
- Static profile secrets are stored locally in `profiles.yaml`.
- OAuth capture blobs are stored locally under `captures/` and `backups/`.
- `ANY_SWITCH_HOME` should stay out of cloud-synced folders such as iCloud
  Drive, Dropbox, OneDrive, and Google Drive; `doctor` reports a warning for
  known sync roots.
- Commands must not print secret fields or capture blob contents.
- Repository commits and release/source packages must not include local agent
  settings, generated manual evidence, smoke-test state, or OS metadata files.
- Target paths must remain inside the current user's home and outside
  `ANY_SWITCH_HOME`.
- OAuth capture operations require the target app to be stopped unless the user
  explicitly uses the documented escape hatch.
