# Runs beside DumbManager

This application does not install over the Python edition.

| Surface | Rust edition |
| --- | --- |
| Window and shortcut | DumbManager Rust |
| Application identifier | `io.dumbmanager.rust` |
| Executable | `dumbmanager-rust.exe` |
| App storage | Tauri application data directory under `io.dumbmanager.rust` |
| Database, settings, exports | Inside the Rust edition's directory |
| WebView profile | Separate profile belonging to the Rust edition |
| Runtime | Rust + Windows WebView2; no Python process |
| Network listener | Off by default; explicit optional phone/Discord listener |
| External accounts | No automatic credential discovery or connection |
| Startup | No startup task or service installed |

No old database, connections, browser profile, tokens, hotkeys, mobile invitation
or T3 selection is imported by opening the app. There is no side effect on the
current application or its GitHub repository. Development verification uses disposable
state. A layout preview server is optional developer tooling and binds only to
loopback; the packaged application does not need it.

Closing the Rust app stops its process. Uninstalling it removes that installation;
retain its data directory if you want to keep its own saved itches. The old edition
continues to use its existing paths and launcher.
