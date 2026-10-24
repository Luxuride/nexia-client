# nextcloud-desktop-rs — repo rules

Rules for agents working in this repository. This file is canonical; the
root-level `AGENTS.md`, `CLAUDE.md` and `GEMINI.md` are symlinks to it.
Edit here only.

## What this repo is

A modern Nextcloud desktop application built with GTK 4 and the Nextcloud Rust SDK.
The repository is split into a workspace of small, focused crates covering core models,
virtual sync / full two-way synchronization engines, and GTK 4 user interface components.

## Structure

- Split the project into multiple libraries wherever a boundary exists.
  Separate concerns do not share a crate: core configuration and events,
  synchronization and VFS engine, and GTK UI are candidate crates in the workspace.
- Prefer a Cargo workspace (`crates/*`) over one growing package as soon as
  a second concern appears. Add a crate instead of a large module.
- Use structs for data, traits for behaviour, and generics for variation.
  Concrete types behind traits, not `match` on variants or `dyn` where a
  generic parameter suffices.
- No long single-file solutions. A file that needs scrolling to navigate is
  split: one type or one concern per file, `mod.rs`/`lib.rs` only re-exports
  and wiring.
- Every non-trivial solution lands as several files with a clear seam
  between them, not as one large file with sections.

## Toolchain

- Build, test, format and lint inside the devcontainer
  (`mcr.microsoft.com/devcontainers/rust:1-bookworm`), never directly relying on host-specific tools.
- Interact with the devcontainer using the `devcontainer` CLI
  (`devcontainer exec --docker-path podman --workspace-folder . <command>`).
- Edition 2024, stable Rust. Do not bump the MSRV without being asked.

## Commands

```sh
cargo build
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

All commands must pass before a change is considered done.
Commit and push each completed feature or logical change to the remote repository.

## Code rules

- No `unsafe` without an explicit request and a safety comment explaining
  the invariant it relies on. Enforce `#![forbid(unsafe_code)]` on all library crates.
- No `unwrap()` / `expect()` / `panic!()` on paths reachable from library
  code. Return `Result` with a typed error; the error enum is part of the
  public API.
- Public items need a doc comment. Hidden or internal items do not, unless
  the intent is non-obvious.
- Missing docs are denied: keep `#![deny(missing_docs)]` in `src/lib.rs`
  satisfied across all crates.
- Errors must name the operation that failed and preserve the source error
  (`#[source]` / `Error::source`); never stringify away the cause.
- Credentials (passwords, app tokens, bearer tokens) must never be
  logged, formatted via `Debug`, or embedded in error messages. Debug impls
  that touch credentials must redact them (`[REDACTED]`).

## Dependencies

- Prefer the standard library and already-present dependencies over adding
  new crates.
- Utilize the Nextcloud Rust SDK (`nextcloud-rust-sdk`) directly, never depending on its lower-level internal crates (`nextcloud-sdk-core`, `nextcloud-sdk-vfs`, `nextcloud-dav`, `nextcloud-auth`, `nextcloud-transport`, etc.).
- Keep wire types and data models distinct from UI presentation state.
- No `*` or floating version requirements; use caret versions.

## Features & File Capabilities

- **Virtual Sync**: Recursively discover remote hierarchies via WebDAV PROPFIND, build and maintain an in-memory `VirtualTree` of metadata placeholders without downloading file contents.
- **On-Demand Hydration**: Download file contents via WebDAV GET only when opened, previewed, or explicitly requested for offline availability.
- **Dehydration**: Release downloaded bytes and cached files back to lightweight metadata placeholders to conserve local disk space.
- **Two-Way Synchronization**: Synchronize selected local directories with remote WebDAV folders, using local and remote scanning, reconciliation, conflict tracking, and journal state persistence.
- **File Operations**: Complete remote file operations including create folder (`MKCOL`), upload file (`PUT`), download file (`GET`), delete (`DELETE`), and rename/move (`MOVE`).

## Tests

- Unit tests live beside the code (`mod tests`). Integration tests live in
  `tests/`.
- File sync and VFS logic must be thoroughly unit tested with mock transport/data without requiring a live Nextcloud server.
- Every bug fix lands with a test that fails before it.

## Style

- Comments explain intent, invariants and non-obvious constraints. Never
  narrate edits, the diff, or the task.
- Rustfmt decides formatting; do not hand-align or fight it.
