# Codex handoff

This document transfers the durable context from the original Codex development task to another computer. It is a sanitized engineering handoff, not a verbatim chat export. Temporary authentication codes, local credentials, caches, and irrelevant personal paths are intentionally excluded.

Last updated: 2026-08-15

## Objective

Turn the official DeepSeek Harness Web workspace into a Windows desktop application that opens from an icon and feels similar to the Codex desktop workflow.

The selected product shape is one Tauri 2 desktop shell with two Windows x64 editions published in the same GitHub Release:

- **Lite Edition** for users who already have a compatible Node.js/npm development environment.
- **Full Edition** for click-to-run use, with pinned Node.js and DeepSeek Harness bundled.

The project is a community companion and must not imply DeepSeek endorsement. The DeepSeek name and whale mark remain the property of their respective rights holders.

## Repository and release state

- Repository: <https://github.com/O1dZ/deepseek-harness-desktop>
- Branch: `main`
- Current desktop version under development: `0.1.2`
- Pinned Harness version: `@deepseek-ai/dsh@0.1.0-rc.6`
- Published releases: none. The broken `v0.1.1` GitHub Release was deleted on 2026-08-15; its Git tag remains for history.
- The v0.1.2 Release workflow intentionally builds only Lite Setup/portable assets and `SHA256SUMS.txt`. Full remains in source but is paused from publication pending renewed real-machine validation.
- The project permits unsigned `.exe` releases. SmartScreen warnings are documented; no paid signing service is required.

The v0.1.1 source fixes were committed as:

- `812ccb5 fix: prevent Windows runtime startup hangs`
- `7b1bbed ci: publish releases with explicit repository`

## Product decisions already made

1. Use Tauri 2 and the system WebView2 instead of Electron to keep the desktop shell small.
2. Reuse the complete official `dsh web` interface. Do not create a separate chat frontend.
3. Load the loopback readiness URL emitted by dsh. Do not load the packaged Web frontend with `loadFile`; the runtime injects boot configuration and provides same-origin API/WebSocket routes.
4. Bind only to `127.0.0.1`.
5. Use a stable persisted high port so Web-origin preferences remain stable. Select another port only when the stored one is occupied.
6. Target Windows x64 first. Other architectures and operating systems are outside the current scope.
7. Keep Lite and Full feature-equivalent. They differ only in Runtime source and host prerequisites.
8. Closing the main window hides the application. Explicit tray **Quit** terminates the Runtime process tree.
9. Keep local logs bounded to 20 MB and seven days, redact secrets, and never upload logs automatically.
10. Do not give the Harness Web origin generic Tauri commands or filesystem access.

## Runtime resolution

### Full Edition

Full resolves absolute paths under packaged resources and starts:

```text
bundled node.exe -> bundled @deepseek-ai/dsh/lib/bin.js -> dsh web
```

It does not depend on system Node.js, npm, browser extensions, or an online npm installation at launch. Full is the recommended release for ordinary click-to-run use.

### Lite Edition

Lite currently resolves the Runtime in this order:

1. User-configured dsh JavaScript entry.
2. Compatible global `@deepseek-ai/dsh` installation.
3. A managed, fixed-version `@deepseek-ai/dsh@0.1.0-rc.6` copy under desktop app data.

If no compatible Runtime exists, Lite automatically runs system Node.js with npm's JavaScript entry and `npm ci` against the complete lock file shared with Full. It installs into a staging directory, validates the dsh version and entry, then atomically renames the result into place. The managed copy persists, so subsequent launches execute dsh directly and do not repeat `npx` dependency resolution. Users never need to run a setup command manually.

The first preparation still downloads the large Harness dependency graph and can take several minutes. It has a separate 30-minute watchdog and overrides a user-level npm `offline=true` setting. Later launches use the local copy and remain pinned until a tested desktop update changes the lock file/version. A compatible global install remains an optional faster path:

```powershell
npm.cmd install --global @deepseek-ai/dsh@0.1.0-rc.6
```

This replaces the v0.1.1 per-launch fixed-version npx fallback, which was observed to spend more than 300 seconds resolving dependencies and consume about 887 MB without ever reaching readiness on the target machine.

## Windows startup incident and fix

The original Lite build remained indefinitely on the startup screen after Workspace selection.

Two independent launch problems were established:

1. Launching `npx.cmd` through nested `cmd.exe /c` quoting produced a stuck command shell with no Node child and no listening port.
2. Direct Node execution through `portable-pty` also hung in ConPTY startup on the target Windows `10.0.26200` build. A minimal `node --version` probe reproduced the hang, proving it was below dsh application startup.

The v0.1.1 fix:

- Removed `portable-pty`/ConPTY.
- Uses `std::process::Command` with hidden stdin/stdout/stderr pipes.
- Invokes npm's `npx-cli.js` directly with Node in Lite fallback mode, avoiding a nested command shell.
- Continuously reads stdout/stderr and waits for the official readiness line:

  ```text
  dsh web: http://127.0.0.1:<port>
  ```

- Keeps all Runtime descendants in a Windows kill-on-close Job Object.
- Adds finite startup watchdogs: 300 seconds for Lite and 120 seconds for Full.
- Shows a diagnostic error instead of spinning forever when readiness is not reported.

Do not restore ConPTY merely to obtain graceful Ctrl+C semantics. On this target system, ConPTY prevented the process from starting at all. Any future graceful-shutdown channel should be designed separately, ideally through an upstream authenticated loopback or named-pipe shutdown mechanism.

## Verification completed for v0.1.1

- Frontend build and Node tests passed.
- Rust unit tests passed.
- `cargo check --features full-runtime` passed.
- Rust formatting passed.
- GitHub CI and security audit jobs passed.
- Lite real cold-start smoke test reached the official readiness line and WebView established loopback connections.
- Full and Lite Setup/portable assets were built and uploaded successfully.
- Setup and portable executables were checked for `0.1.1` version metadata.
- Repository contents were checked for API keys and other publish-blocking secrets before the public release.

## Lite 0.1.2 repair verification

- `npm.cmd run check` passed, including the regression test that rejects an npx fallback and requires the shared lock file.
- Rust formatting, Lite unit tests, default compilation, and `cargo check --features full-runtime` passed.
- A real Windows Lite cold start with no managed Runtime installed completed `npm ci` for 528 locked packages in about 14 seconds, validated and promoted the result, then parsed the official readiness line about 3 seconds later.
- A second launch and the installed-path launch both skipped npm and reached the official readiness line in about 3 seconds.
- Killing the test shell removed the supervised Runtime descendant and released port 7204.
- The official Web root returned HTTP 200 and advertised the Models settings plugin; that plugin was fetched successfully and contains the API-key configuration UI.
- The local installation at `E:\DeepSeekHarness\DeepSeek Harness Desktop Lite` was upgraded in place to 0.1.2 and launched successfully.
- Full was compile-checked only. Its attempted local release build was cancelled at the user's direction, generated files were removed, and the v0.1.2 workflow was restricted to Lite. No 0.1.2 Full asset may be published without renewed real-machine validation.

## Packaging and size observations

The Tauri shell is small; DeepSeek Harness itself is not. The pinned dsh dependency tree observed locally was about 246 MiB before npm cache overhead and contained native modules/binaries.

Measured v0.1.1 local artifacts were approximately:

- Lite Setup: 1.04 MiB
- Lite portable ZIP: 1.19 MiB
- Full Setup: 51.48 MiB
- Full portable directory expanded: 337.41 MiB

Do not remove dynamically loaded Harness plugins merely to reduce package size.

## Branding history

The user requested the DeepSeek whale-style mark shown by an existing local Chrome-installed app shortcut to be used as the project logo. The current project assets are under `branding/` and the generated Tauri icons under `src-tauri/icons/`.

Maintain the README disclaimer that the project is community-built, not officially endorsed, and that the DeepSeek name and mark belong to their respective rights holders. Before wider distribution, re-check the mark's license/trademark permission instead of assuming that technical compatibility grants branding rights.

## Security and publication constraints

- Never commit API keys, tokens, Codex authentication state, GitHub credentials, `.env` files, npm credentials, logs, or signing material.
- Do not copy the entire user `.codex` directory between computers; it can contain personal configuration or credentials.
- Generated directories such as `node_modules/`, `src-tauri/target/`, `.cache/`, `release/`, and `src-tauri/resources/full-runtime/` are intentionally ignored and must be rebuilt.
- Keep the Runtime loopback-only and validate its exact readiness URL.
- GitHub Release checksums establish file integrity but do not remove SmartScreen warnings.

## New-computer continuation procedure

After cloning the repository and installing the documented build prerequisites, open the repository folder in Codex and start with:

```text
Read AGENTS.md and docs/CODEX_HANDOFF.md completely, then inspect the current git status and latest commit. Treat those files as the durable context from the previous Codex task. Do not modify code yet. Summarize the current architecture, release state, known Lite startup tradeoff, and required verification commands, then wait for my next request.
```

For implementation work, follow `AGENTS.md`, preserve unrelated user changes, and keep local build output out of Git.
