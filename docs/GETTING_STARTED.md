# Getting Started

How to get `nextcloud-desktop-rs` building, running and passing its gates.

The short version of the architecture you are about to build: this application is
a **thin GTK 4 shell over [`nextcloud-rust-sdk`](https://github.com/Luxuride/nextcloud-rust-sdk)**.
SYNC, VFS, hydration, retries and WebDAV all live upstream. If you find yourself
writing an HTTP call, a PROPFIND parser or a reconciler in this repository, stop
and check whether the SDK already has it — see [`ARCHITECTURE.md`](ARCHITECTURE.md).

---

## 1. Prerequisites

- **Rust, Edition 2024** (stable toolchain; the workspace uses `resolver = "3"`).
- **GTK 4 + Libadwaita development headers**, plus the usual native build chain.
- A Linux desktop if you want the app bar (`ksni`) — the `tray` feature is Linux-only.

### Devcontainer (recommended)

`.devcontainer/` builds an image with GTK 4, Libadwaita and Rust pre-installed
(`.devcontainer/Dockerfile` starts from
`mcr.microsoft.com/devcontainers/rust:1-bookworm` and apt-installs `pkg-config`,
`libssl-dev`, `libgtk-4-dev`, `libadwaita-1-dev`, `libglib2.0-dev`,
`libdbus-1-dev`, `librsvg2-common`, `adwaita-icon-theme`,
`fonts-noto-color-emoji`, `dbus-x11` and `build-essential`), with the host X11
socket, Wayland socket and session D-Bus bind-mounted in, so `cargo run` works
with the real display forwarded.

It does **not** install FUSE. `fuser 0.18` is a pure-Rust kernel-interface
binding, so the crate compiles without `libfuse`, but `--mount-vfs` only works
when the *host* provides `/dev/fuse` and `fusermount3` (`fuse3` package) and the
container is started with `--device /dev/fuse`. The devcontainer config does not
pass that flag, so FUSE mounting fails inside it by design; run `--mount-vfs` on
the host instead.

This is the recommended way to iterate here: the *host* is frequently missing
`graphene-gobject-1.0` / `gdk-pixbuf`, and `cargo run` then dies in a `-sys` build
script rather than in this repository's code. Note that `initializeCommand` runs
`xhost +SI:localuser:$(id -un)` on the host before the container starts; without
it the forwarded X11 socket has no access.

### Native Linux host

| Distro | Packages |
| --- | --- |
| Fedora / RHEL | `sudo dnf install pkg-config openssl-devel gtk4-devel libadwaita-devel` |
| Debian / Ubuntu | `sudo apt-get install pkg-config libssl-dev libgtk-4-dev libadwaita-1-dev` |

Missing GTK packages show up as `The system library 'graphene-gobject-1.0' ... was
not found` from `graphene-sys`/`gdk-pixbuf-sys` build scripts — install the
`-devel`/`-dev` packages above instead of trying to patch the crate.

---

## 2. Build and run

```sh
# Compile everything — this DOES pull in gtk4/libadwaita, because the default
# features of `nextcloud-desktop-ui` enable them.
cargo check --workspace

# Type-check without GTK headers at all: skip the default features.
cargo check -p nextcloud-desktop-core -p nextcloud-desktop-sync

# Full GTK application
cargo run

# Start hidden in the app bar
cargo run -- --background
cargo run -- -b

# Headless daemon (no display server)
cargo run -- --headless

# CLI/daemon only — no GTK linked at all
cargo run --no-default-features -- --list-mounts
```

### Feature flags

| Feature | Pulls in | Effect |
| --- | --- | --- |
| `default = ["desktop"]` | GTK + tray | The shipping application. |
| `desktop = ["gtk", "tray"]` | — | Convenience umbrella. |
| `gtk` | `gtk4`, `libadwaita`, `glib`, `gdk-pixbuf` | The GTK 4 UI (`nextcloud-desktop-ui/gtk`). |
| `tray` | `ksni` (Linux) | App bar StatusNotifierItem. |
| `android` | `uniffi` | Android bindings. |

Without `gtk`, the binary still builds and falls back to
`nextcloud_desktop_ui::run_headless(...)` instead of failing to launch.

---

## 3. First-run behaviour

On start the binary:

1. Parses `clap` arguments.
2. Initialises `tracing-subscriber` (`RUST_LOG`, default `info`).
3. Opens the encrypted settings database; on failure logs a warning and uses the
   JSON config at `~/.config/nextcloud-desktop-rs/config.json`.
4. Resolves `cache_dir` (`~/.cache/nextcloud-desktop`, falling back to
   `$TMPDIR/nc_cache` when no cache dir can be determined).
5. Builds the `DesktopSyncCoordinator` over an `EventBus` (capacity 256),
   attaches storage, sets the config path and persists config once.
6. Creates the `DesktopUiController`.
7. Calls `register_autostart()` — unconditionally, on every start, regardless of
   the `global.autostart` preference (which is persisted but unread).
8. Calls `controller.ensure_discovered_accounts()` — existing local accounts
   (`nextcloud.cfg`, GNOME Online Accounts, KAccounts) are adopted through the
   SDK's `DesktopSso` discovery here.
9. Launches GTK, or executes the requested CLI operation and returns.

Steps 1–8 always run, CLI flag or not: account discovery happens *before* the
branch that decides between the CLI path and the GTK path, so a one-shot command
still sees discovered accounts. Only the final step differs — see
[`BACKGROUND_SERVICE.md`](BACKGROUND_SERVICE.md) for the flag table.

---

## 4. CLI utilities

Useful one-shot operations for scripting and debugging. All of them skip GTK:

```sh
cargo run -- --sync                    # one two-way cycle
cargo run -- --refresh-vfs             # re-crawl the virtual metadata tree
cargo run -- --hydrate /Documents/Manual.pdf
cargo run -- --dehydrate /Documents/Manual.pdf
cargo run -- --list-mounts             # configured pairs + active orchestrator mounts
cargo run -- --pause-sync
cargo run -- --resume-sync
cargo run -- --set-mount-mode <MOUNT_ID> <MODE>
cargo run -- --mount-vfs [MOUNTPOINT]  # FUSE mount, blocks until Ctrl+C
```

`--hydrate` / `--dehydrate` take **remote** paths (`/Documents/Manual.pdf`), not
local filesystem paths — the coordinator normalises them with
`format!("/{}", path.trim_matches('/'))` before touching the VFS.

---

## 5. Quality gates

All five must pass before a change is proposed — they mirror the jobs in
`.github/workflows/ci.yml`:

```sh
cargo build
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Run them where GTK headers exist (the devcontainer) or the `-sys` crates will fail
before your code is even compiled.

### Where to look when something breaks

| Symptom | Likely cause |
| --- | --- |
| `graphene-sys` / `gdk-pixbuf-sys` build script failure | Missing GTK development packages, or running outside the devcontainer. |
| `failed to get nextcloud-rust-sdk ... you are in the offline mode` | `--offline` was passed while the git dependency is not yet in `~/.cargo/git`: let one online build fetch it first. |
| UI event lag after long uptime | `EventBus` reported `RecvError::Lagged` — expected; the UI re-reads state instead of replaying. |
| A file never hydrates | Check `--list-mounts` for a paused mount, and the `DesktopEvent::NodeHydrationStarted` log line. |
| `--mount-vfs` fails with `Unsupported` or an `ENODEV`/`ENOENT` on `/dev/fuse` | No `/dev/fuse` in this namespace (typical inside the devcontainer), or no `fusermount3` on `PATH`. FUSE mounting is Linux-only: other targets return `io::ErrorKind::Unsupported` from `mount_virtual_fs`. |
| Config edits appear to do nothing | `sync_folders`, `sync_interval_secs`, `global.autostart` and `global.start_minimized` are persisted but not read — see [`CONFIGURATION.md`](CONFIGURATION.md) §3. |
| `--set-mount-mode` does nothing and prints no error | It takes exactly **two** values (`--set-mount-mode <MOUNT_ID> Fuse`; `num_args = 2` at `src/main.rs:61-63`). An **unknown mount id is a silent no-op** — `update_sync_mount_mode` (`account.rs:478-534`) only emits `SyncMountUpdated` inside `if let Some(target) = ... find(|m| m.id == id)`, and returns `Ok(())` otherwise. An unknown mode string is also accepted: it is stored verbatim, and anything that is not `"BidirectionalSync"` is mapped to `MountMode::VirtualVfs`. |
| `--mount-vfs` leaves a stale mountpoint behind | An interrupted run can leave the FUSE mount attached. Unmount with `fusermount3 -u -z <MOUNTPOINT>`; the coordinator also tries this itself before mounting. |
