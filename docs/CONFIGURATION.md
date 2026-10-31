# Configuration Reference

Configuration in `nexia-client` has two layers:

1. **The encrypted settings database** — the authoritative store. It holds the
   whole `AppConfig` plus per-account and global preferences, encrypted with a
   key from the platform keychain.
2. **A JSON fallback** — the same `AppConfig` shape, used when the database
   cannot be opened (missing keyring, read-only data dir, first run in a
   container). Never a second source of truth: the runtime prefers the database
   and only falls back.

Both sides are written by `DesktopSyncCoordinator::persist_config()`, which saves
to the database *and* to the JSON path. The read order at startup is: open the
database, and use `load_app_config()` **only if it returns `Some`**;
on an empty database the JSON is loaded instead and then immediately persisted
into the database, so a pre-existing `config.json` is imported once on first run.
If the database cannot be opened at all, the app runs on JSON alone and logs
`Failed to open encrypted desktop storage, falling back to JSON`.

> **The JSON fallback is not encrypted.** `Account.credentials` serialises
> `password: String` verbatim, so `~/.config/nexia-client/config.json`
> contains the app password in cleartext. Only `desktop_config.db` is
> SQLCipher-encrypted. The file itself is written with mode `0600` through a
> temporary file in the same directory that is renamed into place, so a crash
> mid-write cannot leave a truncated config behind, but the contents are
> still plaintext. Treat the JSON file as secret material, and prefer ensuring
> the keychain works on a real desktop.

---

## 1. Locations

| What | Path |
| --- | --- |
| Settings database | `~/.local/share/nexia/desktop_config.db` |
| Keychain-backed key, local fallback key | `<data dir>/nexia/.storage_key` |
| JSON config fallback | `~/.config/nexia-client/config.json` |
| Cache root (`cache_dir`) | `~/.cache/nexia` |

There is no per-account config file: an account's preferences live inside
`account_preferences` in the database/JSON above and nowhere else. Writing them
to a second location would create a dump that nothing reads back.

The database key is loaded from the keychain
(`KeychainKeyStore::new("eu.luxuride.nexia", "desktop-storage-key")`), with
`.storage_key` next to the database (mode `0600`, created once with `O_EXCL`) as
the fallback when no keyring is available. A key file is not a substitute for a
keyring, only a way to survive its absence.

`DesktopStorage::open_default()` mints a key only when no database exists. For an
existing `desktop_config.db` both candidates — keychain and key file — are
*offered to the database first*: a key is accepted only once it has actually
opened it, so a rotated or stale key is rejected instead of being written back as
the new truth. When neither opens the database, `open_default()` fails with
`CoreError::StorageKeyUnusable` naming the path; the app then falls back to
`config.json` for reads instead of starting over with an empty configuration.

A key file that exists but cannot be read or decoded is an **error**, not a
reason to mint a new key: a fresh key would make the existing
`desktop_config.db` undecryptable, so `read_key_file` returns
`CoreError::ConfigLoad` / `CoreError::StorageKey` naming the path. The same
applies to a missing user data directory: `open_default()` fails with
`CoreError::MissingDataDir` rather than opening a database in the working
directory.

---

## 2. Schema

The JSON fallback is the complete `AppConfig`:

```json
{
  "accounts": [
    {
      "id": "e3a89182-3d75-4b71-bdfc-2794eb8ec381",
      "name": "Work",
      "server": { "server_url": "https://cloud.example.org" },
      "credentials": { "username": "alice", "password": "<app password>" },
      "is_system": false,
      "avatar_path": null
    }
  ],
  "active_account_id": "e3a89182-3d75-4b71-bdfc-2794eb8ec381",
  "sync_folders": [
    {
      "id": "a93df2c1-84e9-4e76-8022-7f8976b32521",
      "account_id": "e3a89182-3d75-4b71-bdfc-2794eb8ec381",
      "local_path": "/home/alice/Nextcloud",
      "remote_path": "/",
      "mode": "VirtualSync",
      "enabled": true
    }
  ],
  "cache_dir": "/home/alice/.cache/nexia",
  "sync_interval_secs": 0,
  "global": {
    "start_minimized": false,
    "autostart": true,
    "default_display": { "folder_items": "DirectOnly", "max_thumbnail_size": 256 }
  },
  "account_preferences": {
    "e3a89182-3d75-4b71-bdfc-2794eb8ec381": {
      "display": { "show_file_size": true },
      "default_view_mode": "Grid",
      "default_zoom_level": "Medium"
    }
  }
}
```

The file is **not** forgiving at the top level. Only `global` and
`account_preferences` carry `#[serde(default)]`; `accounts`,
`active_account_id`, `sync_folders`, `cache_dir` and `sync_interval_secs` are
required, and a missing one makes `load_from_path` fail with
`CoreError::ConfigParse`, which drops the app onto `AppConfig::default()`.

Within `AdditionalItemsDisplayConfig`, only `folder_items`,
`show_direct_item_count`, `show_total_item_count`, `show_direct_folder_size`,
`show_file_size` and `show_last_modified` are serde-defaulted; the other seven
(`show_total_folder_size`, `show_permissions`, `show_mime_type`,
`show_size_breakdown`, `show_disk_usage`, `max_thumbnail_size`,
`autodownload_thumbnails`) must be present in any hand-written block. Copy the
full example above rather than trimming it.

---

## 3. Options

### `accounts: Vec<Account>`

| Field | Meaning |
| --- | --- |
| `id` | `AccountId` — a UUID string, or `"system-account"` for the built-in profile. |
| `name` | Display label. |
| `server.server_url` | Base URL. Normalised through `ServerUrl::parse` with trailing slashes stripped (`ServerConfig::new`). |
| `credentials.username` | Nextcloud login, also the WebDAV path segment. |
| `credentials.password` | App password / token. Redacted as `[REDACTED]` in `Debug` and `Display`. |
| `status` | `Disconnected` \| `Connecting` \| `Connected` \| `Error`. |
| `is_system` | True for the host's built-in immutable account (`Account::system_account()`) — no host or credentials to edit. |
| `avatar_path` | Optional local avatar file (`~/.face`, `~/.face.icon`, or `/var/lib/AccountsService/icons/<user>`). |

### `active_account_id: Option<AccountId>`

Which account the UI opens on. When `None`, the first configured account wins.

### `sync_folders: Vec<SyncFolderConfig>`

| Field | Meaning |
| --- | --- |
| `id` | UUID for the pairing. |
| `account_id` | Owning account. |
| `local_path` | Absolute host path. |
| `remote_path` | Remote folder, e.g. `/` or `/Documents`. |
| `mode` | `VirtualSync` (placeholders, hydrate on demand) or `FullTwoWaySync` (offline copies, local changes pushed). |
| `enabled` | Would be included in a cycle run over this list — but see the caveat below; nothing iterates this vector. |

`SyncFolderConfig::new(account_id, local_path, remote_path, mode)` generates the
`id` and defaults `enabled` to `true`.

> **Caveat — this is not the live mount registry.** `AppConfig.sync_folders` is
> read only by `sync_all_pairs()` (which no caller invokes today) and is
> populated only by tests. The mounts the application actually runs are
> `StoredSyncMount` records: same fields (plus `display_name`, and modes spelled
> `"VirtualVfs"` / `"BidirectionalSync"` / `"Fuse"` as `String`s), stored through
> `Client::store_sync_mount` / `list_sync_mounts` in the SDK's encrypted config
> store and enumerated by `DesktopSyncCoordinator::sync_mounts()`.
>
> The two shapes disagree on `mode` (`SyncMode` enum vs. string) and use different
> ids, so **never edit `sync_folders` expecting mounts to change**. Adding a mount
> goes through `add_sync_mount`; changing one goes through
> `update_sync_mount_mode`. Reconciling the two is unfinished work, not a
> supported workflow.

### `cache_dir: Option<PathBuf>`

Root for hydrated payloads, thumbnails, the persisted VFS tree and per-mount
sync journals, laid out as `accounts/<account_id>/…`. Defaults via
`AppConfig::default_cache_dir()` to `<XDG cache>/nexia`, then
`$TMPDIR/nexia`. `resolved_cache_dir()` is what the rest of the app
calls — do not read `cache_dir` directly.

The launcher additionally fills `cache_dir` in at startup when it is unset, using
`<XDG cache>/nexia` and falling back to `$TMPDIR/nexia_cache`. The two
fallbacks differ (`nexia` vs `nexia_cache` under the temp dir), so an
environment with no resolvable cache dir can land on either path depending on
whether the boot path ran.

### `sync_interval_secs: u64`

Intended period between automatic background cycles. **Persisted but never
read**: `AppConfig` derives `Default`, so the value is `0` unless a JSON file or
the settings database supplies one, and no code in the launcher, the coordinator
or the UI consults it — `--headless` just blocks on `tokio::signal::ctrl_c()`.
Treat it as reserved. Do not describe it as functional until a timer actually
consumes it.

Note the trap: there **is** a periodic loop in the app, but it does not use this
field. `DesktopSyncCoordinator::new` spawns a hardcoded **5 s** remote-etag
poller, and the SDK has its own equally-unread
`OrchestratorConfig.sync_interval_seconds`. Editing `sync_interval_secs` changes
nothing at all; the poll cadence is a literal in the constructor.

### `global: GlobalPreferences`

| Field | Meaning |
| --- | --- |
| `start_minimized` | Persisted "launch hidden" intent. **Not yet read by the launcher** — hiding comes from the `--background` / `-b` flag passed to `launch_gtk_application`. |
| `autostart` | Persisted autostart intent. **Not read by the launcher** — the launcher refreshes an existing autostart entry only when `is_autostart_enabled()` reports one, and never creates or removes one; the settings page owns the on/off decision. |
| `default_display` | Display defaults used when an account has no override. |

Because both booleans are persisted but unused, treat them as reserved fields: do
not assume editing them changes behaviour, and wire them in the launcher before
documenting them as functional.

### Autostart mechanics

`register_autostart()` writes, under `AUTOSTART_APP_NAME =
"eu.luxuride.nexia"`:

| OS | File |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/autostart/eu.luxuride.nexia.desktop` (XDG autostart, `X-GNOME-Autostart-enabled=true`) |
| macOS | `~/Library/LaunchAgents/eu.luxuride.nexia.plist` (`RunAtLoad`) |
| Windows | No-op — returns `Ok(())` |

`unregister_autostart()` and `is_autostart_enabled()` read back the same paths.

### `account_preferences: HashMap<String, AccountPreferences>`

Keyed by the account id string. Per-account override of display settings, plus
`default_view_mode` (`Grid` \| `List`) and `default_zoom_level`
(`Small` \| `Medium` \| `Large` \| `ExtraLarge`).

### `display: AdditionalItemsDisplayConfig`

Controls what the file view shows. Only six of the thirteen fields carry
`#[serde(default)]` — see the note in §2 — so the defaults below are what
`AdditionalItemsDisplayConfig::default()` produces, not what a partial JSON block
is guaranteed to produce:

| Field | Default | Effect |
| --- | --- | --- |
| `folder_items` | `DirectOnly` | `DirectOnly` \| `TotalOnly` \| `Both` |
| `show_direct_item_count` | `true` | "3 items" on folders |
| `show_total_item_count` | `false` | Recursive "124 items" |
| `show_direct_folder_size` | `false` | Direct folder size |
| `show_total_folder_size` | `false` | Recursive folder size |
| `show_file_size` | `true` | File size on items |
| `show_last_modified` | `false` | Modified date on items |
| `show_permissions` | `true` | Permissions in properties/detail views |
| `show_mime_type` | `true` | MIME type |
| `show_size_breakdown` | `true` | Direct vs total breakdown |
| `show_disk_usage` | `true` | Local size vs remote size |
| `max_thumbnail_size` | `256` | Thumbnail cap in pixels |
| `autodownload_thumbnails` | `true` | Fetch thumbnails while browsing |

---

## 4. What lives where

Not everything is `AppConfig`. Knowing the split saves a lot of confusion:

| Data | Store |
| --- | --- |
| Accounts, active account | `AppConfig` (database + JSON) |
| Sync mounts (the live ones) | SDK `ConfigStore` via `Client::store_sync_mount` — per-account `config.db` |
| `AppConfig.sync_folders` | Written by tests and read by the unused `sync_all_pairs()` only |
| Display preferences, view mode, zoom | `AccountPreferences` / `GlobalPreferences` |
| Encrypted per-account SDK state (auth, cache, config DBs) | SDK `StoragePaths` under `accounts/<account_id>/` |
| Sync journals | `accounts/<account_id>/mounts/<mount_id>/journal.json` |
| Virtual tree snapshot | `accounts/<account_id>/vfs/tree.json` |
| Live mount status, pause flags, queue depths | Orchestrator memory only — never persisted |

---

## 5. Using a custom config path

```sh
cargo run -- --config /path/to/custom-config.json
```

`--config` replaces the JSON fallback path and is remembered by the coordinator
(`with_config_path`), so later `persist_config()` writes go to the same file. It
does **not** move the encrypted database, which always lives in the user data
directory.

---

## 6. Precedence and recovery

- Startup: `DesktopStorage::open_default()` → `load_app_config()`; on any failure
  the JSON file is loaded; on failure of that, `AppConfig::default()`.
- The keychain always wins over `.storage_key`, but only among the keys that open
  the existing database; a fresh key is minted only when no database exists.
- A database row that cannot be decoded is repaired from the JSON mirror once and
  logged; the start fails only when the mirror is unusable too, because starting
  empty would persist defaults over the user's accounts.
- `persist_config()` writes both stores and returns the first error, but callers
  in the coordinator log it and continue rather than failing the operation that
  triggered it — the in-memory config is the source of truth for the running
  process. The UI's own settings setters (`set_account_display_config`,
  `set_global_display_config`) propagate the error, so a write that cannot land
  is reported to the user.
- `Account.status` is runtime connection state, not configuration: it is
  `#[serde(skip)]` and `Default`-initialised, so it is never persisted and a
  stored `"status"` key in an old config file is ignored.
