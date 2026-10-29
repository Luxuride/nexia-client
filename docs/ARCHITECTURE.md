# Architecture Overview

`nextcloud-desktop-rs` is a Nextcloud desktop application built in Rust (Edition
2024) and GTK 4 on top of the Nextcloud Rust SDK
([`nextcloud-rust-sdk`](https://github.com/Luxuride/nextcloud-rust-sdk)).

The relationship is one-directional and the SDK does not know this application
exists. **This repository is downstream.** It contributes no protocol code: it
composes SDK handles into a desktop experience and owns only what is genuinely
application-specific — the GTK widget tree, the encrypted settings database, the
per-account cache layout and the app bar daemon.

---

## 1. Design rule: this app is thin on purpose

The previous generation of this application carried its own WebDAV client, VFS
engine, reconciler and sync journal. All of that now lives in the SDK. A change
to how Nextcloud is spoken to belongs upstream in `nextcloud-rust-sdk`; a change
to how the result is presented belongs here.

What this repository owns:

- GTK 4 / Libadwaita widgets, dialogs, navigation, file views, status badges.
- `DesktopSyncCoordinator` — the app-side composition root over SDK handles.
- `AppConfig`, per-account preferences, and the encrypted settings database.
- `DesktopEvent` — the presentation event vocabulary the UI renders.
- Account discovery wiring (system SSO accounts) and the FUSE/desktop glue.

What it must never own:

- A hand-written HTTP client, PROPFIND parser, or retry loop.
- A second reconciler or journal beside `nextcloud_rust_sdk::SyncJournal`.
- A mount registry beside the SDK's `MountsHandle`.
- Dependencies on the SDK's internal crates (`nextcloud-dav`,
  `nextcloud-sdk-core`, `nextcloud-sdk-vfs`, …) — the facade
  `nextcloud_rust_sdk` re-exports everything needed.

---

## 2. Workspace Crate Boundaries

```text
+----------------------------------------------------------------------------------+
| src/main.rs — CLI parsing, tracing, storage bootstrap, GTK or headless launch     |
+----------------------------------------------------------------------------------+
        |                                         |
        v                                         v
+---------------------------------------+  +---------------------------------------+
| nextcloud-desktop-ui  (GTK 4)         |  | nextcloud-desktop-sync                |
|   gtk_app / pages / files / tray      |  |   coordinator, file_ops, folder_sync, |
|   controller (async bridge)           |  |   mounts, account, watcher, fuse_mount|
+---------------------------------------+  +---------------------------------------+
        |                                         |
        v                                         v
+---------------------------------------+  +---------------------------------------+
| nextcloud-desktop-core                |  | nextcloud-rust-sdk (upstream crate)   |
|   account, config, events, storage    |  |   Client, OrchestratorHandle,         |
|   autostart                           |  |   MountsHandle, VfsHandle, sync engine|
+---------------------------------------+  +---------------------------------------+
```

### `crates/nextcloud-desktop-core`

Application domain and persistence. No GTK, no networking.

| Module | Contents |
| --- | --- |
| `account.rs` | `Account`, `AccountId`, `AccountCredentials`, `ServerConfig`, `AccountStatus`. Credential types redact through a custom `Debug`/`Display` (`[REDACTED]`). |
| `config.rs` | `AppConfig` (accounts, `active_account_id`, `sync_folders`, `cache_dir`, `sync_interval_secs`, `global`, `account_preferences`), `SyncFolderConfig`, `SyncMode`, `AdditionalItemsDisplayConfig`, `AccountPreferences`, `GlobalPreferences`. |
| `events.rs` | `EventBus` over `tokio::sync::broadcast` plus the `DesktopEvent` enum. |
| `storage.rs` | `DesktopStorage` — encrypted settings database (`ConfigStore` + `KeychainKeyStore`), with JSON fallback. |
| `autostart.rs` | `register_autostart` / `unregister_autostart` / `is_autostart_enabled` under the app id `eu.luxuride.nextcloud-desktop`. |
| `error.rs` | `CoreError` with `#[source]` preservation. |

### `crates/nextcloud-desktop-sync`

The composition root over the SDK, plus the app-specific layers the SDK
deliberately does not have (thumbnail prioritisation, FreeDesktop preview
installation, FUSE descriptor wiring).

| Module | Contents |
| --- | --- |
| `coordinator.rs` | `DesktopSyncCoordinator` — holds configured accounts, the `Vec<Client>` list, the `NextcloudOrchestrator` + `OrchestratorHandle`, semaphores, thumbnail batch state, FUSE sessions, and the `EventBus`. Also `PriorityPermit` (RAII: while held, background thumbnail work yields) and `ThumbnailQueueSnapshot`. |
| `account.rs` | Per-account `Client` construction (`build_account_client`, deterministic `StoreKey` per account), client lookup, theming cache, mount registry persistence (`sync_mounts`, `add_sync_mount`, `update_sync_mount_mode`, `remove_sync_mount_with_cleanup`), account add/switch/sign-out, and system-account discovery bridging. |
| `file_ops.rs` | The one place remote mutations happen: `refresh_virtual_tree`, `refresh_path`, `hydrate_file`, `hydrate_local_file`, `hydrate_directory`, `dehydrate_file`, `create_directory`, `create_file`, `rename_resource`, `move_resource`, `delete_resource`, public-link sharing, thumbnail fetch/prefetch, cache maintenance, search. |
| `folder_sync.rs` | `FolderSyncEngine` — two-way cycles for local↔remote pairs, delegating discovery to `FolderSyncRunner` and reconciliation to the SDK. |
| `mounts.rs` | Thin wrappers that forward to `MountsHandle` (add/remove/list/pause/resume/sync/status). |
| `watcher.rs` | `FileSystemWatcher` wrapping the SDK watcher; routes local changes to sync and 0-byte reads to hydration. |
| `fuse_mount.rs` | `DesktopHydrator` implementing `AsyncHydrator` over `VfsHandle`, so FUSE reads pull bytes on demand. |
| `freedesktop_thumbs.rs` | Re-exports the SDK's FreeDesktop thumbnail helpers. |
| `engine.rs` | Compatibility re-export of `DesktopSyncCoordinator`, plus the coordinator's integration tests. |
| `error.rs` | `SyncError` with `#[source]` preservation, including the SDK error variants the coordinator surfaces to the UI. |

`lib.rs` re-exports the SDK surface the rest of the workspace is allowed to
touch (`Client`, `MountsHandle`, `VfsHandle`, `OrchestratorHandle`,
`MountDescriptor`, `MountMode`, `SyncJournal`, `reconcile`, …), which is how the
"never import internal SDK crates" rule is enforced in practice.

### `crates/nextcloud-desktop-ui`

GTK 4 presentation, feature-gated so the binary builds without a toolkit.

| Module | Contents |
| --- | --- |
| `controller.rs` | `DesktopUiController` — the async bridge. Every user action is a method here; it calls the coordinator and returns view models or errors. |
| `desktop/gtk_app.rs` | Application window, sidebar navigation stack, window lifecycle (`APPLICATION_ID = "eu.luxuride.nextcloud-desktop"`). |
| `desktop/files/` | File explorer: list view, grid view, path bar, toolbar, status bar, selection, drag-and-drop, context/canvas menus, dialogs (rename, delete, new folder, new item, properties, settings, thumbnails). |
| `desktop/pages/` | Sidebar, activity page, sync folders, settings, add-account/sync dialogs, mount activity and remote-browser dialogs. |
| `desktop/account/` | Account switcher, add-account dialog, sign-out dialog. |
| `desktop/header.rs`, `desktop/mod.rs`, `desktop/tray.rs` | Window header, module wiring, and the `ksni` StatusNotifierItem app bar. |
| `model.rs`, `widget.rs`, `error.rs` | Presentation models, shared widgets, `UiError`. |
| `android/mod.rs` | `uniffi` bindings behind the `android` feature. |

Features: `default = ["desktop"]`, `desktop = ["gtk", "tray"]`,
`gtk` (gtk4 + libadwaita + glib + gdk-pixbuf), `tray` (`ksni`, Linux only),
`android` (`uniffi`).

### Root binary (`src/main.rs`)

`clap` CLI → tracing init → encrypted storage bootstrap (falling back to JSON
when the keyring/database is unavailable) → `DesktopSyncCoordinator` →
`DesktopUiController` → autostart registration → either a one-shot CLI operation,
the headless daemon, or `launch_gtk_application(controller, background)`.

---

## 3. Runtime and concurrency

- **Tokio** owns all I/O. The SDK orchestrator runs on its own OS thread with its
  own runtime; the app never shares that thread's state.
- **Handles, not locks.** The UI holds `OrchestratorHandle`, `MountsHandle` and
  `VfsHandle` and awaits replies. No app-side mutex guards SDK state.
- **Events, not polling.** `EventBus` (`broadcast`, capacity 256) carries
  `DesktopEvent`. The UI subscribes once and re-reads state on
  `RecvError::Lagged` instead of assuming replay.
- **GLib affinity.** GTK work is dispatched onto the default main context with
  `glib::MainContext::default().spawn_local`; Tokio completions hop back through
  channels.
- **Priority permits.** User-initiated work (hydrate, sync, directory listing)
  takes a `PriorityPermit` — an RAII guard over an `AtomicUsize` plus a `Notify`.
  While the counter is non-zero, background thumbnail workers yield and pause, so
  the UI stays responsive under a cold cache.
- **Bounded concurrency.** `preview_semaphore` (8 permits) and
  `download_semaphore` (4 permits) cap simultaneous preview and payload fetches;
  `hydrate_file` acquires a download permit before touching the network.

### Account switching repopulates UI state

Account-scoped state is never cached across a switch. `switch_account`
(`account.rs`) drops the cached theming snapshot (`clear_cached_theme`) before it
emits `AccountSwitched`, and `cached_theme_logo` (`account.rs:83`) only matches
logo files prefixed with the *active* server's authority, so branding cannot
leak from one account to another. In the UI, every account-sensitive surface
subscribes to `AccountSwitched`/`AccountSignedOut` and re-reads its own state:

| Surface | Re-reads on switch |
| --- | --- |
| `desktop/pages/sidebar.rs` | Banner title, slogan, colour, logo, status dot. |
| `desktop/header.rs` | Window title. |
| `desktop/account/switcher.rs` | Active account name and avatar. |
| `desktop/pages/settings.rs` | Active-account row and sign-out target. |
| `desktop/pages/sync_folders.rs` | Mount list and global pause state. |
| `desktop/pages/activity.rs` | Mount rows, activity rows, thumbnail counters. |
| `desktop/files/page.rs` | Search query cleared, view refreshed. |
| `desktop/gtk_app.rs` | `reload_account_view_state()` — navigation root, persisted view mode, zoom, and the root directory's sort criteria. |

The themed CSS lives in exactly one `CssProvider` registered at
`STYLE_PROVIDER_PRIORITY_USER`; `apply_server_theme_color` reloads its rules and
`clear_server_theme_color` empties them, so the static `APPLICATION`-priority
defaults from `gtk_app.rs` take over again instead of competing with stale
providers.

### `DesktopEvent` variants

`SyncStarted`, `TransferProgress`, `FileTransferCompleted`, `NodeHydrationStarted`,
`NodeHydrated`, `NodeDehydrated`, `ThumbnailDownloadStarted`, `ThumbnailReady`,
`ThumbnailFailed`, `ThumbnailProgress`, `DisplaySettingsChanged`, `VfsTreeUpdated`,
`SyncCompleted`, `MountProgress`, `AccountSwitched`, `AccountSignedOut`,
`SyncMountAdded`, `SyncMountUpdated`, `SyncMountRemoved`, `Notification`.

---

## 4. Mount modes

A mount's mode is stored as a **string** on `StoredSyncMount`, not as an enum, and
that string is what this app dispatches on. Two of the three values correspond to
an SDK `MountMode` whose transfer semantics the SDK implements:

| Mode | Behaviour |
| --- | --- |
| `VirtualVfs` | Metadata tree with placeholders; bytes fetched on demand via `VfsHandle::hydrate_local`. Exposed either as an in-app browser tree or as a real FUSE mount. |
| `BidirectionalSync` | Local folder mirrored against a remote folder; discovery, reconciliation and journaling delegated to the SDK. |

The third persisted string is `"Fuse"`. The SDK does have a `MountMode::Fuse`
variant, but it is only a label — the orchestrator never branches on it — so this
app does **not** register a `MountDescriptor` for a Fuse mount at all. Instead
`mount_fuse_folder` (`crates/nextcloud-desktop-sync/src/fuse_mount.rs:169-210`)
creates the mountpoint, unmounts any stale session (`unmount_fuse_folder`, then
`fusermount3 -u -z`), and calls `nextcloud_rust_sdk::mount_virtual_fs` itself with
a `FuseDriver::with_hydrator_and_root(shared_tree, DesktopHydrator, remote_path)`,
handing the driver the current Tokio runtime when one is available. The returned
`BackgroundSession` is kept in `fuse_sessions: HashMap<String, _>` keyed by mount
id, and a background pass preloads any already-cached FreeDesktop thumbnails
(`fuse_mount.rs:196-211`). Dropping that map entry — or `unmount_all_fuse()` — is
what unmounts. When a future `add_mount`/`update_sync_mount_mode` does build a
descriptor, `"BidirectionalSync"` maps to `MountMode::BidirectionalSync` and
everything else — `"VirtualVfs"`, `"Fuse"`, a typo — maps to
`MountMode::VirtualVfs` (`account.rs:508-511`). The persisted string, not the
`MountMode` enum, is what this app actually dispatches on.

**Who drives a cycle.** The SDK ships no timer — `OrchestratorConfig`
`sync_interval_seconds` is stored but unread, and `MountsHandle::sync_mount()`
only reconciles the local directory against the journal. The real two-way cycle
is therefore this app's job: `FolderSyncEngine::sync_mount(mount_id)`
(`crates/nextcloud-desktop-sync/src/folder_sync.rs:47`) takes a priority permit,
resolves the active DAV context, creates the local target if missing, then awaits
`FolderSyncRunner::execute_sync_cycle(&dav_client, &user, is_vfs, is_cancelled,
on_progress)`. It brackets the cycle with `DesktopEvent::SyncStarted` /
`SyncCompleted` and `MountProgress`. Its skip rules are worth knowing exactly:

| Condition | Result |
| --- | --- |
| id starts with `app-` (`folder_sync.rs:49-51`) | `Ok(0)` — internal VFS mount |
| mount paused, or global `is_sync_paused()` (`:56-67`) | `Err(SyncError::OperationDisabled("Synchronization is currently paused"))` |
| mount missing, `!enabled`, or `mode == "Fuse"` (`:70-73`) | `Ok(0)` |

So a paused mount is the one case that surfaces an error; every other skip is a
silent zero.

On success it returns `outcome.downloaded + outcome.uploaded`, and after the
cycle it re-runs `discover_remote` to `batch_prefetch_thumbnails_with_ids` (batch
256) for every non-directory entry matching `is_thumbnail_candidate` — a suffix
list covering `jpg/jpeg/png/gif/webp/svg/bmp/tiff/mp4/mkv/webm/mov/avi/pdf`.
A cancellation observed after the cycle returns early, skipping both the
thumbnail pass and the closing `MountProgress`/`SyncCompleted` events. It is
reached from

| Trigger | Call path |
| --- | --- |
| User action (`--sync`, the app bar's *Sync All Folders Now*, a settings button) | `DesktopUiController::trigger_sync()` → `coordinator.sync_all_mounts()` → `FolderSyncEngine::sync_all_mounts()` (`folder_sync.rs:204`) |
| A mount is added | `nextcloud-desktop-sync/src/account.rs:404-409` → `sync_mount_folder(id)` (`tokio::spawn`, fire-and-forget) |
| A mount is resumed | `nextcloud-desktop-sync/src/mounts.rs:114` (inside `resume_mount`) → `sync_mount_folder(id)` (`tokio::spawn`, fire-and-forget) |
| The filesystem watcher sees a local edit | `crates/nextcloud-desktop-sync/src/watcher.rs:28-35` → guarded by `!is_mount_syncing(&mount_id)` → `sync_mount_folder(id)` |

Those four are not the only source of activity. The coordinator also starts a
**remote-change poller** (`coordinator.rs:250-262`): a `tokio` task that sleeps
2 s, runs one check, then loops every **5 s hardcoded** calling
`check_remote_changes_all_accounts()` (`file_ops.rs:1044-1101`). That function,
per account, does a **Depth: 0 PROPFIND asking only for `d:getetag`** on the DAV
root, compares the etag against `last_root_etags`, and only when it changed calls
`refresh_account_virtual_tree(account)` (Depth: 1 `PropfindBody::all()`) followed
by `sync_account_mounts(account_id)`. So an idle session does poll the server
every 5 s per account; the *expensive* work only happens on an etag change, and
the first check after startup always counts as a change (the map is empty).

That loop is the only periodic timer in the app, it is not configurable, and it
is **not** driven by `AppConfig.sync_interval_secs` or the SDK's
`sync_interval_seconds` — both are stored and unread.

The watcher itself is installed by `DesktopSyncCoordinator::init_watcher()`
(`coordinator.rs:584-633`), which the constructor schedules 500 ms after startup
(`coordinator.rs:183-189`) and which account switching calls again
(`account.rs:700`). It is idempotent (`if watcher_guard.is_none()`) and per mount
either registers a FUSE session (`mode == "Fuse"`) or adds a `MountDescriptor` to
the orchestrator *and* calls `watch_folder(local_path, mount_id)`. It also removes
the internal `app-default-vfs` mount on every call.

**The per-account virtual drive is a real mount record.** `sync_mounts()`
(`account.rs:263-335`) is not a plain list: it purges any internal mount that is
not the active account's `app-vfs-<account_id>`, deletes the legacy
`mounts/app-default-vfs` cache directory, keeps only records whose `account_id`
is empty or matches the active account, then inserts a `VirtualVfs` record with
`display_name: "Application Virtual Drive"` if the account has none, and finally
deduplicates by id. `is_internal_vfs(id)` is `id.starts_with("app-") || id ==
"app-default-vfs"` (`account.rs:258-261`), which is what keeps these out of the
sync loop and out of the removable list in the UI.

**Mount records.** The authoritative per-mount record is `StoredSyncMount` in the
SDK's encrypted config store (`id`, `account_id`, `remote_path`, `local_target`,
`mode`, `display_name`, `enabled`, `created_at`, `updated_at`), written via
`Client::store_sync_mount` and read via `list_sync_mounts`. `AppConfig.sync_folders`
uses a different shape (`SyncMode` enum) and is *not* the live registry — see
[`CONFIGURATION.md`](CONFIGURATION.md) §3. Mounts whose id starts with `app-` are
internal VFS mounts and are skipped by the sync loop.

---

## 5. Storage layout

| Path | Contents |
| --- | --- |
| `~/.config/nextcloud-desktop-rs/config.json` | `AppConfig` fallback when the encrypted database is unavailable. |
| `~/.local/share/nextcloud-desktop/desktop_config.db` | Encrypted settings database (SDK `ConfigStore`; key from the platform keychain, `.storage_key` fallback). |
| `~/.local/share/nextcloud-desktop/.storage_key` | Hex `StoreKey` written only when the keychain is unavailable (`DesktopStorage::resolve_key`). |
| `~/.config/nextcloud-desktop-rs/accounts/<account_id>.json` | Write-only per-account `AccountPreferences` dump — never read back. |
| `<cache_dir>/accounts/<account_id>/` | SDK `StoragePaths` (per-account auth/cache/config databases). |
| `<cache_dir>/accounts/<account_id>/vfs/tree.json` | Persisted virtual tree snapshot. |
| `<cache_dir>/accounts/<account_id>/mounts/<mount_id>/journal.json` | Per-mount sync journal. |
| `<cache_dir>/accounts/<account_id>/thumbnails/` | Cached thumbnails. |
| `<cache_dir>/theming/` | Cached server logo assets. |

`<cache_dir>` defaults to `~/.cache/nextcloud-desktop` and is per-account
subdivided so switching accounts never mixes caches.

---

## 6. Credential confidentiality invariant

Passwords, app tokens and basic-auth headers never appear in logs, terminal
output, crash reports or `Debug` output.

- `AccountCredentials` hand-writes `Debug`
  (`crates/nextcloud-desktop-core/src/account.rs:57-65`) to wrap the password in
  `nextcloud_rust_sdk::SecretString` — its `Debug` renders
  `SecretString("<redacted>")`. Its `Display` (`:67-71`) prints the username only.
  The struct derives `Clone`, `PartialEq`, `Eq`, `Serialize` and `Deserialize` but
  **not** `Debug`/`Display`, which is what forces the hand-written impls.
- `ServerConfig` holds no secret (it derives `Debug`; the field is just
  `server_url`), so there is nothing to redact there.
- Secrets cross the SDK boundary as `SecretString`; `AccountCredentials::secret()`
  is the deliberate, named escape hatch.

The invariant is about **rendering**, not about at-rest encoding. Note the
asymmetry: `Account.credentials` derives `Serialize`/`Deserialize` with a plain
`password: String`, so the JSON fallback (`~/.config/nextcloud-desktop-rs/config.json`)
contains the app password in cleartext, while the encrypted `desktop_config.db`
holds it inside SQLCipher. Treat that JSON file as secret material.
