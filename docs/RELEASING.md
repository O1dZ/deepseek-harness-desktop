# Releasing

## Versioning

Lite and Full source currently share the same desktop version. The pinned Harness and portable Node.js versions are declared in source and printed in diagnostics. Full publication is temporarily disabled; v0.1.2 publishes Lite only. Restoring Full assets requires both Edition test matrices and real Windows smoke tests to pass.

## Local release checks

**PowerShell：**

```powershell
npm.cmd ci
npm.cmd run check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml --features full-runtime
```

## Create a release

Update the version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`, then push a matching tag.

**PowerShell：**

```powershell
git tag v0.1.0
git push origin v0.1.0
```

The Release workflow currently builds only the Lite installer and portable archive, then generates `SHA256SUMS.txt` and creates the GitHub Release. Full remains in source but must not be added back to the workflow until its release validation is complete.

## Signing policy

Unsigned builds remain supported. They function normally but can trigger SmartScreen and may be blocked by managed enterprise policy. The preferred free path is SignPath Foundation if the project qualifies. Signing secrets must live in GitHub Secrets and must never be committed.

Checksums provide reproducibility evidence but do not suppress SmartScreen warnings.

## Size budgets

- Lite Setup: warning above 20 MB
- Full Setup: warning above 220 MB
- Full installed footprint: investigate above 450 MB

Size regressions should be explained in release notes. Do not remove dynamically loaded Harness plugins merely to reduce the package.
