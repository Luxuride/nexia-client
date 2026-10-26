# nextcloud-desktop-rs — repo rules

Rules for agents working in this repository. This file is canonical; the
root-level `AGENTS.md`, `CLAUDE.md` and `GEMINI.md` are symlinks to it.
Edit here only.

## What this repo is

A Nextcloud desktop application built with GTK 4 on top of the Nextcloud Rust SDK
(`nextcloud-rust-sdk`). The repository is a workspace of small, focused crates:
core models, the coordinator that composes SDK handles, and GTK 4 user interface
components.

**This repository is a consumer, and downstream of the SDK.** The SDK is the
engine and does not know this application exists: it holds no reference to this
repo, names no client, and is not modified to serve a client's needs. Consequences
for work here:

- Protocol behaviour (WebDAV, VFS state, hydration bookkeeping, journals,
  reconciliation, retries) belongs upstream in the SDK. Do not re-implement it
  here, and do not work around an SDK limitation with a parallel implementation —
  fix the SDK or file it.
- A change that only exists to satisfy this UI is a change this repository owns:
  widgets, view models, `DesktopEvent`s, coordinator policy, configuration
  schema, cache layout, the app bar.
- Never describe this app as the SDK's reason to exist, and never add SDK-side
  knowledge of it (feature flags, config fields, branches on "is this client").
  Anything of that shape is a review rejection.
- The one-way rule is mechanically enforced: only the `nextcloud_rust_sdk`
  facade crate may be imported. Internal SDK crates are off limits (see
  Dependencies).

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

## Documentation

Development docs live in `docs/` and are part of the deliverable, not an
afterthought:

| File | Covers |
| --- | --- |
| `docs/ARCHITECTURE.md` | Crate boundaries, runtime/concurrency model, storage layout |
| `docs/GETTING_STARTED.md` | Toolchain, feature flags, gates, troubleshooting |
| `docs/BACKGROUND_SERVICE.md` | App bar, background lifespan, CLI/daemon flags |
| `docs/CONFIGURATION.md` | `AppConfig` schema, two-layer persistence, precedence |
| `docs/VIRTUAL_SYNC.md` | Hydration/dehydration policy, mount modes, FUSE |

When a change alters a public shape — a config field, a CLI flag, a
`DesktopEvent` variant, a crate boundary, a storage path — update the matching
doc in the same change. Docs describe what the code does today: no aspirational
sections, no documenting a field the code never reads. If a flag or field is
persisted but unimplemented, say so explicitly in the doc.

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

Capabilities are surfaced here, not implemented here. The SDK owns the mechanism;
this repository owns the presentation and the policy around it.

- **Virtual Sync**: browse remote hierarchies as metadata placeholders without
  downloading bodies. Refresh is an SDK crawl driven by
  `DesktopSyncCoordinator::refresh_virtual_tree()`; the app persists the tree
  snapshot and renders it.
- **On-Demand Hydration**: bodies are fetched only when opened, previewed or
  requested offline. The coordinator fetches bytes and hands them to
  `VfsHandle::hydrate_local`; the SDK owns the state transition.
- **Dehydration**: release downloaded payloads back to placeholders. Delegated to
  `VfsHandle::dehydrate_local`.
- **Two-Way Synchronization**: local↔remote pairs with journal-backed change
  tracking. Discovery lives in `FolderSyncEngine`; reconciliation, conflict
  handling and `SyncJournal` belong to the SDK.
- **File Operations**: create folder, upload, download, delete, rename/move,
  share links, thumbnails and search — each a coordinator method that calls an
  SDK client, never a hand-written HTTP request.

If a capability is missing, check the SDK's public surface before writing any
code here.

## Tests

- Unit tests live beside the code (`mod tests`). Integration tests live in
  `tests/`.
- File sync and VFS logic must be thoroughly unit tested with mock transport/data without requiring a live Nextcloud server.
- Every bug fix lands with a test that fails before it.

## Style

- Comments explain intent, invariants and non-obvious constraints. Never
  narrate edits, the diff, or the task.
- Rustfmt decides formatting; do not hand-align or fight it.
