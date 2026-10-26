# Contributing to nextcloud-desktop-rs

Thank you for your interest in contributing to Nextcloud Desktop in Rust!

This codebase is structured as a high-integrity Cargo workspace of small, focused crates where every crate's public interface is treated as a strict API contract.

Both **human** and **LLM / AI-assisted contributions** are welcome!

**Know where the code belongs before you write it.** This repository is a
consumer of [`nextcloud-rust-sdk`](https://github.com/Luxuride/nextcloud-rust-sdk)
and the SDK does not depend on it in any way. Protocol handling, VFS state,
hydration bookkeeping, journals, reconciliation and retries are SDK concerns; a
PR that re-implements any of them here will be asked to move. Widgets, view
models, `DesktopEvent`s, coordinator policy, configuration, cache layout and the
app bar are this repository's concerns. See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

---

## 1. Toolchain & Quality Gates

All development, testing, formatting, and linting must be executed in stable Rust (Edition 2024), inside the project devcontainer (`mcr.microsoft.com/devcontainers/rust:1-bookworm`).

Before submitting any Pull Request, all commands must pass with zero errors and zero warnings:

```sh
cargo build
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

---

## 2. Core Code Rules

1. **No `unsafe` code**: `#![forbid(unsafe_code)]` is enforced across all library crates unless explicitly requested with an accompanying safety comment explaining the invariant it relies on.
2. **Never panic in library code**: No `unwrap()`, `expect()`, or `panic!()` on paths reachable from library code. Return typed `Result<T, Error>`.
3. **Missing documentation is denied**: Every public struct, enum, function, method, trait, and module must have a clear doc comment (`#![deny(missing_docs)]`).
4. **Credential confidentiality**: Passwords, app tokens, and basic-auth headers must never be logged, formatted via `Debug` or `Display`, or embedded in error messages. Redact credentials (e.g. `[REDACTED]`) and wipe secret buffers where possible.
5. **Typed, causal errors**: Errors must name the operation that failed and preserve the source error via `#[source]` / `Error::source`. Never stringify away causes.
6. **Workspace crate boundaries**: Separate concerns do not share a crate:
   - `crates/nextcloud-desktop-core`: Models, configuration, accounts, encrypted storage, event bus, autostart.
   - `crates/nextcloud-desktop-sync`: `DesktopSyncCoordinator` over SDK handles, file operations, folder sync, watcher, FUSE hydration.
   - `crates/nextcloud-desktop-ui`: GTK 4 views, widgets, dialogs, controller, app bar.
7. **SDK facade only**: Import `nextcloud_rust_sdk`, never its internal crates
   (`nextcloud-sdk-core`, `nextcloud-sdk-vfs`, `nextcloud-dav`, `nextcloud-auth`,
   `nextcloud-transport`, …). If the facade is missing something, that is an SDK
   issue, not a reason to reach around it.
8. **No single-file monoliths**: Split files by type or concern. `lib.rs` / `mod.rs` only handles re-exports and wiring.
9. **Docs ship with the code**: A change that alters a config field, CLI flag,
   `DesktopEvent` variant, crate boundary or storage path updates the matching
   file in `docs/` in the same PR. Docs describe current behaviour only.

---

## 3. Pull Request Workflow

1. Follow Conventional Commits:
   - `feat(sync): add dehydration command for hydrated placeholders`
   - `fix(vfs): preserve directory etags during metadata refresh`
   - `chore: update dependencies`
2. Ensure all quality gates pass in the devcontainer.
3. Update the relevant `docs/` file if a documented surface changed.
