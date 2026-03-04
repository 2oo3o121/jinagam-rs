# jinagam

`jinagam` is a small Windows tray app for **multi-monitor** setups that shows a glow overlay when the mouse crosses a monitor boundary.

By [EunsuKim03](https://github.com/EunsuKim03).

## Features

- Runs in the system tray
- Shows a colored boundary glow across multi-monitor edges
- Lets you change color, width, duration, span mode, and performance mode from the tray menu

## Installation

For installation, download the packaged build from the `v1.0.0` release.

- Download `jinagam_v1.0.0.zip` [here](https://github.com/2oo3o121/jinagam-rs/releases/tag/v1.0.0)

## Stack

- Rust
- Windows API via the `windows` crate
- Native Windows tray integration
- Native overlay rendering on Windows

## Run

```bash
cargo run
```

## Build

```bash
cargo build --release
```

The app icon is embedded into the executable, so the built `.exe` can be distributed as a single file.
