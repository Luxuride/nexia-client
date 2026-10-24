# Contributing to nextcloud-desktop-rs

Thank you for your interest in contributing to Nextcloud Desktop in Rust!

This codebase is structured as a high-integrity Cargo workspace of small, focused crates where every crate's public interface is treated as a strict API contract.

Both **human** and **LLM / AI-assisted contributions** are welcome!

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
   - `crates/nextcloud-desktop-core`: Models, configuration, accounts, event bus.
   - `crates/nextcloud-desktop-sync`: Virtual sync engine (metadata tree & on-demand hydration), local folder synchronizer, and remote WebDAV file operations.
   - `crates/nextcloud-desktop-ui`: GTK 4 desktop application views, widgets, and dialogs.
7. **No single-file monoliths**: Split files by type or concern. `lib.rs` / `mod.rs` only handles re-exports and wiring.

---

## 3. Pull Request Workflow

1. Follow Conventional Commits:
   - `feat(sync): add dehydration command for hydrated placeholders`
   - `fix(vfs): preserve directory etags during metadata refresh`
   - `chore: update dependencies`
2. Ensure all quality gates pass in the devcontainer.
