# Security Policy

## Supported versions

Only the latest GitHub Release receives security fixes while DeepSeek Harness remains in Developer Preview.

## Reporting

Do not open a public issue for a vulnerability that could expose API credentials, local files, command execution, or the loopback Runtime. Use the repository's private security advisory form after the GitHub project is published.

## Security model

- Harness listens only on loopback.
- The official web origin receives no Tauri IPC permissions.
- Secrets and full environment dumps are redacted from desktop logs.
- Logs stay local and are never uploaded automatically.
- Full pins its Runtime; Lite warns before using an unverified Runtime.
- The Runtime process tree is attached to a Windows Job Object.

Unsigned releases can show Windows SmartScreen warnings. Download only from this repository's GitHub Releases and verify `SHA256SUMS.txt`.
