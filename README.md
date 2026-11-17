# Orbshacker Desktop

A Windows desktop app built with Rust and Tauri. It includes Discord game search,
manual executable entry, Steam quest mode, and a session queue with a numeric
countdown. New installations default to 16 minutes per session. You can change
the duration in Settings.

## Requirements

- Windows with the Microsoft Edge WebView2 Runtime
- Rust with the MSVC toolchain
- Visual Studio C++ Build Tools and a Windows SDK

## Run or build

Run these commands from the repository root.

```powershell
cargo run --locked -p orbshacker
cargo build --release --locked -p orbshacker
```

The release executable is `target/release/orbshacker.exe`. Cargo embeds the
interface in the executable. The app stores `settings.json` next to the
executable when you save preferences.

## Project layout

- `ui/` contains the HTML, CSS, and JavaScript interface.
- `src-tauri/` contains the desktop window, native commands, and app assets.
- `crates/core/` contains session logic, the native countdown window, and tests.

## Tests

```powershell
cargo test --workspace --locked
```

See [LICENSE](LICENSE) for license terms and attribution.
