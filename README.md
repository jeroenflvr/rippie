# rippie

Clipboard bridge from a Linux/Wayland UTM VM back to the macOS host, over a shared directory.

UTM already shares the clipboard from mac to VM. rippie covers the other direction:

- `rippie-linux` (VM) listens for Wayland clipboard changes and writes them to `<dir>/.rippie/clipboard`.
- `rippie-mac` (host) polls that file and loads changes into the mac clipboard.

Supported formats, all carried together when the source offers them:

| Linux mime type                                 | macOS pasteboard type    |
|-------------------------------------------------|--------------------------|
| `text/plain;charset=utf-8`, `UTF8_STRING`       | `public.utf8-plain-text` |
| `text/html`                                     | `public.html`            |
| `text/rtf`, `application/rtf`                   | `public.rtf`             |
| `image/png` (else `image/tiff`, `image/jpeg`, `image/gif`) | matching image type |

Writes are atomic (temp file + rename), so the mac side never reads a partial buffer.
The file is a sequence of records: `<mime>\n<length>\n<bytes>`.

## Requirements

- A directory shared between the mac and the VM (UTM shared directory).
- A Wayland compositor that supports the `wlr-data-control` protocol
  (Sway, Hyprland, KDE Plasma, and other wlroots based compositors; not GNOME).

## Install

Download the archive for your platform from GitHub Releases:

| Binary         | Platform                                              |
|----------------|-------------------------------------------------------|
| `rippie-mac`   | `aarch64-apple-darwin`, `x86_64-apple-darwin`         |
| `rippie-linux` | `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` |

Linux binaries are statically linked.

## Usage

Both binaries take one argument: the shared directory, as seen from that side.

On the mac:

```sh
rippie-mac /Users/me/shared
```

In the VM:

```sh
rippie-linux /mnt/shared
```

`rippie-linux` creates `<dir>/.rippie` if it does not exist.

## Run as a service

Requires [go-task](https://taskfile.dev). Builds, installs and starts the service.

- macOS: binary in `~/.local/bin` (override with `BIN_DIR=...`), LaunchAgent `~/Library/LaunchAgents/rippie.plist`.
- Linux: binary in `/usr/bin`, unit `/etc/systemd/system/rippie.service` (uses sudo). The service runs as
  `RUN_USER` (default: current user) against that user's Wayland socket `WAYLAND_DISPLAY` (default: `wayland-0`).

```sh
# on the mac
task deploy DIR=/Users/me/shared

# in the VM
task deploy DIR=/mnt/shared
```

Logs: `/tmp/rippie.log` on macOS, `journalctl -u rippie` on Linux.

## Build

```sh
task build       # binary for this OS
task build:all   # both binaries; rippie-linux for aarch64-unknown-linux-musl
task build:all LINUX_TARGET=x86_64-unknown-linux-musl
```

Outputs go to `target/release/` and `target/<target>/release/`.

## Release

```sh
task release VERSION=v0.1.0
```

Pushes the tag. GitHub Actions builds all targets and publishes them with `SHA256SUMS` to GitHub Releases.
