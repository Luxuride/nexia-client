# Nextcloud Desktop (Rust / GTK 4)

A modern Nextcloud desktop application built with GTK 4 and the Nextcloud Rust SDK ([`nextcloud-rust-sdk`](https://github.com/Luxuride/nextcloud-rust-sdk)), featuring **virtual synchronization** with on-demand file hydration, bidirectional local folder synchronization, and comprehensive remote file operations.

---

## Key Features

- **Virtual Sync Engine**:
  - Recursively fetches and monitors remote hierarchies via WebDAV `PROPFIND` without consuming local disk space.
  - Maintains an in-memory `VirtualTree` of metadata-only placeholders backed by [`nextcloud-sdk-vfs`].
  - **On-Demand Hydration**: Downloads file content via WebDAV `GET` only when opened, previewed, or explicitly requested.
  - **Dehydration**: Safely releases cached payload bytes back to lightweight placeholders to conserve disk space.
- **Two-Way Synchronization**:
  - Full bidirectional sync for configured local directories mapped to remote Nextcloud folders.
  - Local filesystem and WebDAV hierarchy scanners with mtime and ETag tracking.
  - Journal state persistence (`SyncJournal`) and intelligent change reconciliation (`SyncAction`).
- **Feature-Complete File Operations**:
  - Upload (`PUT`), download (`GET`), create directory (`MKCOL`), delete (`DELETE`), move/rename (`MOVE`), and copy (`COPY`).
- **Modern GTK 4 Interface**:
  - Navigation sidebar: Cloud Files (Virtual), Local Sync Folders, Transfers & Activity, Settings.
  - File browser with breadcrumb navigation and real-time status badges (☁️ Virtual Placeholder, 💾 Offline Ready, 🔄 Syncing).
  - Contextual actions: Hydrate, Dehydrate, Open, Upload, New Folder, Rename, Delete.
- **Strict Confidentiality**:
  - Redaction of authentication secrets and app tokens across all `Debug`, `Display`, and logging paths (`[REDACTED]`).

---

## Workspace Architecture

Following the development guidelines of `nextcloud-rust-sdk`:

```
crates/
├── nextcloud-desktop-core/   # Accounts, credentials, configuration, and event bus
├── nextcloud-desktop-sync/   # VFS virtual sync engine, two-way sync, and WebDAV file operations
└── nextcloud-desktop-ui/     # GTK 4 UI views, navigation stack, and async controllers
src/
└── main.rs                   # Entry point and CLI runner
```

---

## Toolchain & Quality Gates

Development is designed to be executed inside the devcontainer (`mcr.microsoft.com/devcontainers/rust:1-bookworm`):

```sh
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

---

## CLI Usage

```sh
# Launch the GTK 4 desktop application
cargo run

# Refresh the virtual sync file tree from Nextcloud
cargo run -- --refresh-vfs

# Hydrate a virtual file on-demand
cargo run -- --hydrate /Documents/Report.pdf

# Dehydrate a virtual file to free local disk space
cargo run -- --dehydrate /Documents/Report.pdf

# Run an immediate bidirectional sync cycle
cargo run -- --sync

# Headless daemon mode
cargo run -- --headless
```

---

## License

GPL-3.0-or-later
