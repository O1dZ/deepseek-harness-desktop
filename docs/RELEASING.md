# Releasing

## Versioning

Lite and Full always share the same desktop version. The pinned Harness and portable Node.js versions are declared in source and printed in diagnostics. A Harness upgrade requires both Edition test matrices to pass.

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

The Release workflow builds both Edition installers and portable archives, then generates `SHA256SUMS.txt` and creates the GitHub Release.

## Signing policy

Unsigned builds remain supported. They function normally but can trigger SmartScreen and may be blocked by managed enterprise policy. The preferred free path is SignPath Foundation if the project qualifies. Signing secrets must live in GitHub Secrets and must never be committed.

Checksums provide reproducibility evidence but do not suppress SmartScreen warnings.

## Size budgets

- Lite Setup: warning above 20 MB
- Full Setup: warning above 220 MB
- Full installed footprint: investigate above 450 MB

Size regressions should be explained in release notes. Do not remove dynamically loaded Harness plugins merely to reduce the package.
