# Configuration Reference

Nextcloud Desktop persists configuration profiles in a standard JSON format under the user's config directory:

```
~/.config/nextcloud-desktop-rs/config.json
```

---

## 1. Configuration Schema

```json
{
  "accounts": [
    {
      "id": "e3a89182-3d75-4b71-bdfc-2794eb8ec381",
      "name": "My Nextcloud Server",
      "server": {
        "server_url": "https://cloud.example.org"
      },
      "credentials": {
        "username": "alice",
        "password": "app-token-generated-from-nextcloud"
      },
      "status": "Connected"
    }
  ],
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
  "cache_dir": "/home/alice/.cache/nextcloud-desktop",
  "sync_interval_secs": 300
}
```

---

## 2. Configuration Options

### `accounts`
An array of configured Nextcloud user accounts:
- **`id`**: Unique UUID identifying the account profile.
- **`name`**: User-defined display name.
- **`server.server_url`**: Nextcloud base URL (e.g. `https://cloud.example.com`). Trailing slashes are stripped automatically.
- **`credentials.username`**: Nextcloud login username.
- **`credentials.password`**: App password or authentication token.

### `sync_folders`
An array of configured folder pairings:
- **`id`**: Unique UUID for the pairing.
- **`account_id`**: Associated account UUID.
- **`local_path`**: Absolute path on the host filesystem.
- **`remote_path`**: Relative path on the remote Nextcloud instance (e.g. `/` or `/Documents`).
- **`mode`**: Synchronization mode:
  - `"VirtualSync"`: Metadata-only placeholders; downloads file bodies on-demand.
  - `"FullTwoWaySync"`: Bidirectional file synchronization maintaining offline copies.
- **`enabled`**: Whether this pairing is included during periodic sync cycles.

### `cache_dir`
Directory used for storing hydrated virtual files and thumbnails. Defaults to `~/.cache/nextcloud-desktop`.

### `sync_interval_secs`
Interval in seconds between automatic background sync cycles. Defaults to `300` (5 minutes).

---

## 3. Custom Configuration File

A custom configuration path can be supplied at launch using the `--config` flag:

```sh
cargo run -- --config /path/to/custom-config.json
```
