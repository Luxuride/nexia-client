# Nextcloud Desktop (Rust / GTK 4)

A Nextcloud desktop application in Rust and GTK 4, built on
[`nextcloud-rust-sdk`](https://github.com/Luxuride/nextcloud-rust-sdk): virtual
synchronisation with on-demand hydration, two-way folder sync, and a real file
browser over the remote hierarchy.

```sh
cargo run              # GTK 4 application
cargo run -- --background   # start hidden in the app bar
cargo run -- --headless     # daemon, no display server
```

---

## What this repository is

**The SDK is the engine; this is the app.** Every protocol-level operation —
WebDAV requests, PROPFIND parsing, VFS state, hydration bookkeeping, journals,
reconciliation, retries, rate limiting — is implemented in `nextcloud-rust-sdk`.
This repository supplies what a desktop application must add on top:

- the GTK 4 / Libadwaita UI,
- the encrypted settings database and per-account cache layout,
- the app bar daemon and autostart integration,
- `DesktopSyncCoordinator`, the composition root that turns UI intent into SDK
  handle calls and SDK events into renderable state.

The dependency runs one way. The SDK does not know this application exists, and
nothing in this repository may re-implement something the SDK already owns — see
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the rule and how the workspace
enforces it (only the `nextcloud_rust_sdk` facade is importable; internal SDK
crates are off limits).

---

## Features

- **Virtual sync with on-demand hydration** — metadata-only placeholders, bodies
  fetched on open or on request, dehydrated again to reclaim space. Optionally
  exposed as a real FUSE mount.
- **Two-way folder sync** — local↔remote pairs with journal-backed change
  tracking and reconciliation delegated to the SDK.
- **Full file operations** — upload, download, create folder, delete, rename,
  move, share links, thumbnails, search.
- **Multi-account** — per-account SDK clients, caches and preferences, plus SSO
  account discovery through the SDK (`nextcloud.cfg`, GNOME Online Accounts,
  KAccounts).
- **Desktop behaviour** — StatusNotifierItem app bar (`ksni`), hide-to-background,
  autostart, CLI/daemon modes that run without a display server.
- **Confidentiality invariant** — credentials and app tokens never reach logs,
  `Debug` output or crash reports; secrets render as `[REDACTED]`.

---

## Workspace layout

```text
crates/
├── nextcloud-desktop-core/   # Accounts, config, encrypted storage, events, autostart
├── nextcloud-desktop-sync/   # DesktopSyncCoordinator, file ops, folder sync, watcher, FUSE
└── nextcloud-desktop-ui/     # GTK 4 views, navigation, controller, tray, Android bindings
src/
└── main.rs                   # CLI parsing, bootstrap, GTK / headless launch
docs/
├── ARCHITECTURE.md           # Crate boundaries, runtime model, storage layout
├── GETTING_STARTED.md        # Toolchain, features, gates, troubleshooting
├── BACKGROUND_SERVICE.md     # App bar, background lifespan, CLI/daemon modes
├── CONFIGURATION.md          # AppConfig schema, two-layer persistence, precedence
└── VIRTUAL_SYNC.md           # Hydration policy, mount modes, FUSE exposure
```

---

## Documentation

| Document | Read it when |
| --- | --- |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | You need the crate boundaries, event model, storage layout, or to decide whether a change belongs here or upstream. |
| [`docs/GETTING_STARTED.md`](docs/GETTING_STARTED.md) | You are building for the first time or hit a build/`-sys` failure. |
| [`docs/BACKGROUND_SERVICE.md`](docs/BACKGROUND_SERVICE.md) | You are touching the tray, window lifecycle, or the CLI/daemon flags. |
| [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md) | You are adding a setting or wondering where a value is persisted. |
| [`docs/VIRTUAL_SYNC.md`](docs/VIRTUAL_SYNC.md) | You are working on hydration, dehydration, mount modes or FUSE. |

---

## Development

```sh
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

All five must pass (`cargo build` included). Run them inside `.devcontainer/` — the image ships GTK 4 and
Libadwaita, and a bare host usually lacks `graphene-gobject-1.0`, which fails in a
`-sys` build script rather than in this repository's code. The container does not
forward `/dev/fuse`, so `--mount-vfs` is a host-side operation — see
[`docs/GETTING_STARTED.md`](docs/GETTING_STARTED.md) §1.

Contributions from humans and AI-assisted contributors are both welcome; see
[`CONTRIBUTING.md`](CONTRIBUTING.md), plus [`agents/AGENTS.md`](agents/AGENTS.md)
for the mandatory-startup and documentation rules that apply to every change.

---

## CLI

```sh
cargo run -- --refresh-vfs                    # re-crawl the virtual metadata tree
cargo run -- --hydrate /Documents/Report.pdf  # hydrate one remote path
cargo run -- --dehydrate /Documents/Report.pdf
cargo run -- --sync                           # one two-way cycle
cargo run -- --list-mounts                    # configured pairs + live mount status
cargo run -- --mount-vfs ~/NextcloudVfs       # FUSE mount, blocks until Ctrl+C
cargo run -- --headless                       # daemon
```

Paths are remote paths. See
[`docs/BACKGROUND_SERVICE.md`](docs/BACKGROUND_SERVICE.md) for the full table.

---

## License

GPL-3.0-or-later
