# Codex repository guidance

Before changing this repository, read these files in order:

1. `docs/CODEX_HANDOFF.md`
2. `README.md`
3. `docs/ARCHITECTURE.md`
4. `CONTEXT.md`
5. `SECURITY.md`

## Durable constraints

- Target Windows x64 first. Do not add other architectures unless explicitly requested.
- Preserve both Lite and Full editions. They share one Tauri 2 shell and user data.
- The desktop app supervises the official `dsh web` UI; do not reimplement the Harness frontend.
- Keep the Harness Runtime bound to `127.0.0.1` and do not grant its remote origin Tauri IPC capabilities.
- Do not replace the current hidden standard-pipe process launch with ConPTY. ConPTY startup hung on the target Windows build and was removed deliberately.
- Full must use its pinned bundled Node.js and dsh runtime. Lite must prefer a custom entry or a compatible global dsh installation.
- Treat Lite's fixed-version `npx` fallback as a known product tradeoff. It is convenient but cold starts are slow; changing it requires an explicit product decision and tests.
- Do not commit credentials, `.env` files, logs, caches, generated runtimes, build output, signing certificates, or release binaries.
- Releases may remain unsigned. Never introduce a paid signing requirement.
- Follow the vocabulary in `CONTEXT.md` in user-facing copy.

## Required verification

Run these commands before handing off a code change:

```powershell
npm.cmd run check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml --features full-runtime
```

For runtime-launch changes, also perform a real Windows smoke test for both editions and confirm that the official readiness line is parsed before WebView navigation.
