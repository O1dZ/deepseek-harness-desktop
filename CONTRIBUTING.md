# Contributing

Issues and pull requests are welcome once the repository is published. Keep the product vocabulary in [CONTEXT.md](CONTEXT.md) and record only durable, non-obvious architectural decisions under `docs/adr/`.

## Development requirements

- Windows 10 22H2 or Windows 11 x64
- Node.js 24
- Rust stable with the MSVC target
- Microsoft Visual C++ Build Tools
- WebView2 Runtime

## Checks

**PowerShell：**

```powershell
npm.cmd ci
npm.cmd run check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
```

Do not commit generated Full Runtime files, npm caches, build output, API keys, logs, or signing certificates.
