# Getting Started

Welcome to **Nextcloud Desktop** in Rust! This guide walks you through setting up the development environment, building the application, and running it locally or inside the devcontainer.

---

## 1. Prerequisites & Toolchain

The project requires **Rust Edition 2024** (stable).

### Recommended: Using Dev Containers
The repository includes a complete devcontainer setup with pre-installed GTK 4, Libadwaita, and native development libraries:
1. Open this repository in VS Code, Antigravity, or any devcontainer-compatible editor.
2. Select **"Reopen in Container"** when prompted (or via Command Palette).
3. The container builds from [`.devcontainer/Dockerfile`](file:///home/lux/Projects/nextcloud-dekstop-rs/.devcontainer/Dockerfile) and is immediately ready for compilation and GUI testing.

### Local Development
If building directly on a Linux host, ensure development packages are installed:
- **Fedora / RHEL**:
  ```sh
  sudo dnf install pkg-config openssl-devel gtk4-devel libadwaita-devel
  ```
- **Debian / Ubuntu**:
  ```sh
  sudo apt-get install pkg-config libssl-dev libgtk-4-dev libadwaita-1-dev
  ```

---

## 2. Building & Running

### Launch the Desktop GUI
```sh
cargo run
```

### Launch Minimized in Background (App Bar)
```sh
cargo run -- --background
```

### Run in Headless Daemon Mode
```sh
cargo run -- --headless
```

---

## 3. Command Line Utilities

The desktop binary provides one-off CLI commands for scripting and automated maintenance:

```sh
# Refresh the virtual file tree from Nextcloud
cargo run -- --refresh-vfs

# Hydrate an individual virtual file on-demand
cargo run -- --hydrate /Documents/Manual.pdf

# Dehydrate a file back to a lightweight placeholder
cargo run -- --dehydrate /Documents/Manual.pdf

# Trigger immediate bidirectional sync on configured local pairs
cargo run -- --sync
```

---

## 4. Quality Gates & Testing

To ensure code stability and adherence to development guidelines, all four commands must pass before submitting changes:

```sh
# Run unit tests
cargo test --workspace

# Check formatting
cargo fmt --all -- --check

# Run linter
cargo clippy --all-targets -- -D warnings

# Verify documentation coverage
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```
