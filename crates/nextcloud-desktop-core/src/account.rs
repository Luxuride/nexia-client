//! Account model and credential structures with strict confidentiality.

use serde::{Deserialize, Serialize};
use std::fmt;
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

/// Authentication secret containing user credentials.
///
/// Implements custom [`fmt::Debug`] and [`fmt::Display`] to strictly redact
/// secret content from logs or traces.
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
        f.debug_struct("AccountCredentials")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
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
    /// Base URL of the Nextcloud instance (e.g. `https://cloud.example.com`).
    pub server_url: String,
}

impl ServerConfig {
    /// Creates a new server configuration ensuring stripped trailing slashes and authority credentials.
    pub fn new(server_url: impl Into<String>) -> Self {
        let mut url = server_url.into();
        while url.ends_with('/') {
            url.pop();
        }
        if let Some(scheme_idx) = url.find("://") {
            let scheme = &url[..scheme_idx + 3];
            let rest = &url[scheme_idx + 3..];
            if let Some(at_idx) = rest.find('@') {
                url = format!("{scheme}{}", &rest[at_idx + 1..]);
            }
        }
        Self { server_url: url }
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
    pub avatar_path: Option<std::path::PathBuf>,
}

impl Account {
    /// Detects the host desktop username, display name, and avatar picture path.
    pub fn detect_host_user_info() -> (String, String, Option<std::path::PathBuf>) {
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "system".to_string());

        let home = dirs::home_dir();
        let display_name = match std::env::var("NAME") {
            Ok(n) if !n.trim().is_empty() => n,
            _ => {
                let capitalized = if let Some(first) = user.chars().next() {
                    format!("{}{}", first.to_uppercase(), &user[first.len_utf8()..])
                } else {
                    user.clone()
                };
                format!("{capitalized} (System)")
            }
        };

        let avatar_candidates = [
            home.as_ref().map(|h| h.join(".face")),
            home.as_ref().map(|h| h.join(".face.icon")),
            Some(std::path::PathBuf::from(format!(
                "/var/lib/AccountsService/icons/{user}"
            ))),
        ];

        let avatar_path = avatar_candidates
            .into_iter()
            .flatten()
            .find(|p| p.is_file());

        (user, display_name, avatar_path)
    }

    /// Returns the built-in, immutable host system account profile with discovered user details.
    pub fn system_account() -> Self {
        let (username, display_name, avatar_path) = Self::detect_host_user_info();
        Self {
            id: AccountId("system-account".to_string()),
            name: display_name,
            server: ServerConfig::new("https://cloud.nextcloud.com"),
            credentials: AccountCredentials::new(username, ""),
            status: AccountStatus::Connected,
            is_system: true,
            avatar_path,
        }
    }

    /// Creates a system-managed account profile (e.g. discovered from GNOME Online Accounts or KDE).
    pub fn system_sso_account(
        id: impl Into<String>,
        name: impl Into<String>,
        server: ServerConfig,
        credentials: AccountCredentials,
        avatar_path: Option<std::path::PathBuf>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_redaction() {
        let creds = AccountCredentials::new("alice", "super_secret_token_123");
        let debug_str = format!("{creds:?}");
        assert!(!debug_str.contains("super_secret_token_123"));
        assert!(debug_str.contains("[REDACTED]"));

        let display_str = format!("{creds}");
        assert!(!display_str.contains("super_secret_token_123"));
    }

    #[test]
    fn server_config_url() {
        let srv = ServerConfig::new("https://mycloud.org/");
        assert_eq!(srv.server_url, "https://mycloud.org");
        assert_eq!(
            srv.webdav_url("bob"),
            "https://mycloud.org/remote.php/dav/files/bob"
        );
    }
}
