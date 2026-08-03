<div align="center">

# MovieBox-TUI

Search, browse, play, and download movies and series from a keyboard-first terminal interface.

[![Crates.io](https://img.shields.io/crates/v/moviebox-tui.svg?logo=rust)](https://crates.io/crates/moviebox-tui)
[![CI](https://github.com/mesamirh/MovieBox-Tui/actions/workflows/ci.yml/badge.svg)](https://github.com/mesamirh/MovieBox-Tui/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

<img src="https://raw.githubusercontent.com/mesamirh/MovieBox-Tui/main/assets/screenshots/01-home-blocky.jpg" alt="MovieBox-TUI home screen" width="85%">

</div>

## Features

- MovieBox and 4KHDHub catalogs with isolated search, details, stream, and image caches
- Movie, series, anime, episode, quality, and subtitle browsing
- Playback through mpv, VLC, or IINA
- HTTP header forwarding for protected playback sources
- Resumable downloads with progress, retry, cancellation, and partial-file preservation
- Sequential full-season download queue with one subtitle-language preference
- IPTV playlists, categories, search, and local playlist configuration
- Kitty, Sixel, iTerm2, and text poster rendering where supported
- Configurable themes, update checks, and automatic cache cleanup

MovieBox-TUI resolves links from upstream services. Availability can change when those services change.

## Requirements

- 64-bit Windows, macOS, or Linux
- Terminal size of at least 85×24
- One supported player: mpv, VLC, or IINA
- Internet connection

Release binaries currently cover:

| Platform | Architectures |
| --- | --- |
| macOS | Intel and Apple Silicon universal binary |
| Linux | x86_64 and ARM64 static musl binaries |
| Windows | x86_64 and ARM64 |

## Installation

### Homebrew — macOS or Linux

```bash
brew tap mesamirh/moviebox-tui https://github.com/mesamirh/MovieBox-Tui
brew install moviebox-tui
```

The formula selects the correct macOS, Linux x86_64, or Linux ARM64 release.

### Install script — macOS or Linux

```bash
curl -fsSL https://raw.githubusercontent.com/mesamirh/MovieBox-Tui/main/install.sh | bash
```

The script detects OS and CPU architecture, verifies the release SHA-256 checksum, then installs to `/usr/local/bin`. Without write access or `sudo`, it uses `~/.local/bin`.

### PowerShell — Windows

```powershell
irm https://raw.githubusercontent.com/mesamirh/MovieBox-Tui/main/install.ps1 | iex
```

The installer selects x86_64 or ARM64, verifies SHA-256, installs under `%LOCALAPPDATA%\MovieBox-Tui`, and adds that directory to the user PATH. Open a new terminal after first installation.

### Cargo

Requires Rust 1.90 or newer:

```bash
cargo install moviebox-tui --locked
```

### Build from source

```bash
git clone https://github.com/mesamirh/MovieBox-Tui.git
cd MovieBox-Tui
cargo build --release --locked
```

Binary location: `target/release/moviebox-tui` (`moviebox-tui.exe` on Windows).

## Player setup

MovieBox-TUI checks standard application locations, PATH executables, and Linux Flatpak installations.

Detected automatically:

- macOS: `/Applications`, `~/Applications`, Homebrew/PATH
- Linux: PATH, Flatpak mpv, Flatpak VLC
- Windows: PATH, common Program Files locations, Microsoft Store aliases

Portable or custom installations can be selected with environment variables:

| Player | Variable |
| --- | --- |
| mpv | `MOVIEBOX_MPV_PATH` |
| VLC | `MOVIEBOX_VLC_PATH` |
| IINA | `MOVIEBOX_IINA_PATH` |

macOS/Linux example:

```bash
export MOVIEBOX_MPV_PATH="$HOME/Apps/mpv"
moviebox-tui
```

Windows PowerShell example:

```powershell
$env:MOVIEBOX_VLC_PATH = "D:\Apps\VLC\vlc.exe"
moviebox-tui
```

IINA is macOS-only. mpv provides the broadest source-header compatibility.

## Usage

Run:

```bash
moviebox-tui
```

### Main controls

| Key | Action |
| --- | --- |
| Arrow keys | Navigate lists, grids, seasons, episodes, and dialogs |
| Enter | Open or confirm selection |
| Esc / `b` | Close dialog or go back |
| `o` | Choose another player |
| `d` | Download selected episode or season |
| `r` | Refresh current content |
| `Ctrl+P` | Switch content provider |
| `Ctrl+T` | Toggle IPTV mode |
| `?` | Show help |
| `q` | Quit |

### Commands

| Command | Action |
| --- | --- |
| `/discover`, `/home` | Open discovery view |
| `/movies` | Browse movies |
| `/shows` | Browse series |
| `/anime` | Browse anime |
| `/list` | Show IPTV channels |
| `/config` | Configure IPTV playlists |
| `/update` | Check for a newer release |
| `/toggle-update` | Enable or disable automatic update checks |
| `/clear-cache` | Remove cached application data |

`/update` checks availability and shows the release location; it does not replace the running binary. Re-run the installer or Homebrew upgrade command to update.

## Downloads

Downloads are stored under the operating system Downloads directory:

```text
MovieBox-TUI/
├── Movies/
└── Series/<title>/Season <number>/
```

Season downloads run sequentially to limit disk and network pressure. After choosing a subtitle language for the first episode, the same language is requested for remaining episodes when available. Missing subtitles do not discard completed video files.

Interrupted downloads preserve `.part` and resume metadata files. Starting the same download again attempts to resume it.

## Configuration and cache

MovieBox-TUI uses standard OS directories:

| Platform | Configuration | Cache |
| --- | --- | --- |
| Linux | `${XDG_CONFIG_HOME:-~/.config}/moviebox-tui` | `${XDG_CACHE_HOME:-~/.cache}/moviebox-tui` |
| macOS | `~/Library/Application Support/moviebox-tui` | `~/Library/Caches/moviebox-tui` |
| Windows | `%APPDATA%\moviebox-tui` | `%LOCALAPPDATA%\moviebox-tui` |

Catalog providers use separate cache namespaces. Expired or invalid cache entries are discarded automatically; files older than seven days are cleaned at startup.

## Updating

Homebrew:

```bash
brew update
brew upgrade moviebox-tui
```

Script installations: run the same install command again. Cargo installations:

```bash
cargo install moviebox-tui --locked --force
```

## Uninstallation

Homebrew:

```bash
brew uninstall moviebox-tui
brew untap mesamirh/moviebox-tui
```

Script installation:

```bash
sudo rm -f /usr/local/bin/moviebox-tui
rm -f "$HOME/.local/bin/moviebox-tui"
```

Windows PowerShell:

```powershell
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\MovieBox-Tui"
```

Cargo:

```bash
cargo uninstall moviebox-tui
```

Configuration and cache directories remain until removed manually.

## Troubleshooting

### No media player found

Confirm player runs from terminal:

```bash
mpv --version
vlc --version
```

For portable installs, set corresponding `MOVIEBOX_*_PATH` variable.

### Images do not render

Image support depends on terminal protocol. Text UI remains usable without graphics. Resize/focus crashes involving Sixel should be reported with OS, terminal name, and `moviebox-tui --version`.

### Linux command not found after script install

Add local binary directory to shell PATH:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

Persist that line in shell profile when needed.

## Development

Before submitting changes:

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo audit
cargo package --locked
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidance.

## Legal

MovieBox-TUI does not host media. It is not affiliated with MovieBox, 4KHDHub, IPTV providers, player projects, or terminal vendors. Users are responsible for complying with laws and service terms applicable to them.

## License

Licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
