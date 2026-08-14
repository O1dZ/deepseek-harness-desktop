# Use one Tauri shell with two runtime sources

Both editions will use the same Tauri 2 shell and the system Evergreen WebView2 runtime. Lite resolves a compatible external Node.js and Harness runtime, while Full carries a tested portable Node.js and Harness runtime; sharing the shell avoids duplicating window, tray, security, and process-supervision behavior, and the installer can bootstrap WebView2 on the uncommon machine where it is absent.
