//! Account model and connection profile representations.

use nexia_nextcloud_sdk::{SecretString, ServerUrl};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use uuid::Uuid;

/// Unique identifier for an account profile.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(pub String);

impl AccountId {
    /// Generates a new random unique account identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for AccountId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Authentication credentials with redacted debug output.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountCredentials {
    /// Username or email address.
    pub username: String,
    /// App password, token, or session password.
    pub password: String,
}

impl AccountCredentials {
    /// Creates a new credential pair.
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    /// Access the underlying password secret.
    pub fn secret(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for AccountCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sec = SecretString::new(&self.password);
        f.debug_struct("AccountCredentials")
            .field("username", &self.username)
            .field("password", &sec)
            .finish()
    }
}

impl fmt::Display for AccountCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AccountCredentials(username: {})", self.username)
    }
}

/// Server connection configuration details.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Base URL of the Nextcloud instance.
    pub server_url: String,
}

impl ServerConfig {
    /// Creates a new server configuration.
    pub fn new(server_url: impl Into<String>) -> Self {
        let url = server_url.into();
        let cleaned = ServerUrl::parse(&url)
            .map(|u| u.as_str().trim_end_matches('/').to_string())
            .unwrap_or(url);
        Self {
            server_url: cleaned,
        }
    }

    /// Computes the WebDAV endpoint URL for a given username.
    pub fn webdav_url(&self, username: &str) -> String {
        format!("{}/remote.php/dav/files/{}", self.server_url, username)
    }
}

/// State of an account connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountStatus {
    /// Account is disconnected or idle.
    Disconnected,
    /// Account is currently connecting or authenticating.
    Connecting,
    /// Account is authenticated and online.
    Connected,
    /// Authentication or connection error encountered.
    Error,
}

/// Account profile representing a configured Nextcloud connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Unique identifier.
    pub id: AccountId,
    /// Display name or label.
    pub name: String,
    /// Server connection configuration.
    pub server: ServerConfig,
    /// Credentials for authentication.
    pub credentials: AccountCredentials,
    /// Current connection status.
    pub status: AccountStatus,
    /// Whether this is the built-in system account (immutable).
    #[serde(default)]
    pub is_system: bool,
    /// Path to local avatar image if available.
    #[serde(default)]
    pub avatar_path: Option<PathBuf>,
}

impl Account {
    /// Returns the built-in, immutable host system account profile with discovered user details.
    pub fn system_account() -> Self {
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "system".to_string());

        let home = dirs::home_dir();
        let avatar_path = [
            home.as_ref().map(|h| h.join(".face")),
            home.as_ref().map(|h| h.join(".face.icon")),
            Some(PathBuf::from(format!(
                "/var/lib/AccountsService/icons/{user}"
            ))),
        ]
        .into_iter()
        .flatten()
        .find(|p| p.is_file());

        Self {
            id: AccountId("system-account".to_string()),
            name: format!("{user} (System)"),
            server: ServerConfig::new("https://cloud.nextcloud.com"),
            credentials: AccountCredentials::new(user, ""),
            status: AccountStatus::Connected,
            is_system: true,
            avatar_path,
        }
    }

    /// Creates a system-managed account profile.
    pub fn system_sso_account(
        id: impl Into<String>,
        name: impl Into<String>,
        server: ServerConfig,
        credentials: AccountCredentials,
        avatar_path: Option<PathBuf>,
    ) -> Self {
        Self {
            id: AccountId(id.into()),
            name: name.into(),
            server,
            credentials,
            status: AccountStatus::Connected,
            is_system: true,
            avatar_path,
        }
    }

    /// Creates a new account profile with default disconnected status.
    pub fn new(
        name: impl Into<String>,
        server: ServerConfig,
        credentials: AccountCredentials,
    ) -> Self {
        Self {
            id: AccountId::new(),
            name: name.into(),
            server,
            credentials,
            status: AccountStatus::Disconnected,
            is_system: false,
            avatar_path: None,
        }
    }
}
