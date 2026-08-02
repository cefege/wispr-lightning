//! Deepgram API key storage in the app-support directory.
//!
//! This is deliberately file-only. It never opens macOS Keychain or Windows
//! Credential Manager, so saving or reading the key cannot trigger an OS
//! credential prompt.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use thiserror::Error;

/// Account holding the user's Deepgram API key.
pub const DEEPGRAM_API_KEY: &str = "deepgram-api-key";

const CREDENTIAL_FILE: &str = "credentials.json";

#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("credential file {path}: {source}")]
    File {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("credential file {path} is not valid JSON: {source}")]
    Corrupt {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

pub type Result<T, E = CredentialError> = std::result::Result<T, E>;

/// Thread-safe access to the local credentials file.
#[derive(Debug)]
pub struct CredentialStore {
    file: FileStore,
}

impl Default for CredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialStore {
    pub fn new() -> Self {
        Self::file_backed(wl_core::paths::app_support_dir().join(CREDENTIAL_FILE))
    }

    pub fn file_backed(path: PathBuf) -> Self {
        Self {
            file: FileStore::new(path),
        }
    }

    /// The stored value for `account`, or `None` when nothing is stored.
    pub fn get(&self, account: &str) -> Result<Option<String>> {
        self.file.get(account)
    }

    pub fn set(&self, account: &str, value: &str) -> Result<()> {
        self.file.set(account, value)
    }

    /// Remove `account`. Deleting something that was never stored succeeds.
    pub fn delete(&self, account: &str) -> Result<()> {
        self.file.delete(account)
    }
}

/// A JSON object of account to secret, stored owner-readable only.
#[derive(Debug)]
struct FileStore {
    path: PathBuf,
    /// Serializes read-modify-write so two concurrent `set`s cannot lose one
    /// another's account.
    write: Mutex<()>,
}

impl FileStore {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            write: Mutex::new(()),
        }
    }

    fn get(&self, account: &str) -> Result<Option<String>> {
        Ok(self.read_all()?.remove(account))
    }

    fn set(&self, account: &str, value: &str) -> Result<()> {
        let _guard = self.lock();
        let mut all = self.read_all()?;
        all.insert(account.to_owned(), value.to_owned());
        self.write_all(&all)
    }

    fn delete(&self, account: &str) -> Result<()> {
        let _guard = self.lock();
        let mut all = self.read_all()?;
        if all.remove(account).is_none() {
            return Ok(());
        }
        self.write_all(&all)
    }

    /// A poisoned lock only means some other thread panicked mid-write; the map
    /// is re-read from disk on every operation, so there is no corrupt state to
    /// protect against.
    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.write.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn read_all(&self) -> Result<BTreeMap<String, String>> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(source) => {
                return Err(CredentialError::File {
                    path: self.path.clone(),
                    source,
                })
            }
        };
        serde_json::from_slice(&bytes).map_err(|source| CredentialError::Corrupt {
            path: self.path.clone(),
            source,
        })
    }

    /// Writes through a temporary file created 0600 and then renamed, so the
    /// secret is never briefly world-readable and a crash mid-write cannot
    /// truncate the existing store.
    fn write_all(&self, all: &BTreeMap<String, String>) -> Result<()> {
        let dir = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir).map_err(|source| CredentialError::File {
            path: dir.to_path_buf(),
            source,
        })?;

        let json = serde_json::to_vec_pretty(all).map_err(|source| CredentialError::Corrupt {
            path: self.path.clone(),
            source,
        })?;

        let tmp = self.path.with_extension("tmp");
        write_private(&tmp, &json).map_err(|source| CredentialError::File {
            path: tmp.clone(),
            source,
        })?;
        std::fs::rename(&tmp, &self.path).map_err(|source| CredentialError::File {
            path: self.path.clone(),
            source,
        })
    }
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Windows has no mode bits. `%APPDATA%` is already per-user and inherits an
/// ACL that excludes other non-administrator users, which is the same guarantee
/// 0600 buys on unix.
#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    const TEST_ACCOUNT: &str = "another-account";

    fn temp_store() -> (CredentialStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("wl-creds-{}", uuid::Uuid::new_v4()));
        let path = dir.join(CREDENTIAL_FILE);
        (CredentialStore::file_backed(path.clone()), path)
    }

    #[test]
    fn a_secret_that_was_never_stored_reads_as_absent_rather_than_failing() {
        let (store, _) = temp_store();
        assert_eq!(store.get(DEEPGRAM_API_KEY).expect("get"), None);
    }

    #[test]
    fn a_stored_secret_reads_back_verbatim() {
        let (store, _) = temp_store();
        store
            .set(DEEPGRAM_API_KEY, "dg-key-\u{00e9}\n")
            .expect("set");
        assert_eq!(
            store.get(DEEPGRAM_API_KEY).expect("get").as_deref(),
            Some("dg-key-\u{00e9}\n")
        );
    }

    #[test]
    fn accounts_do_not_overwrite_one_another() {
        let (store, _) = temp_store();
        store.set(TEST_ACCOUNT, "other-secret").expect("set");
        store.set(DEEPGRAM_API_KEY, "api-key").expect("set");
        assert_eq!(
            store.get(TEST_ACCOUNT).expect("get").as_deref(),
            Some("other-secret")
        );
        assert_eq!(
            store.get(DEEPGRAM_API_KEY).expect("get").as_deref(),
            Some("api-key")
        );
    }

    #[test]
    fn deleting_removes_only_the_named_account_and_tolerates_a_missing_one() {
        let (store, _) = temp_store();
        store.set(TEST_ACCOUNT, "other-secret").expect("set");
        store.set(DEEPGRAM_API_KEY, "api-key").expect("set");

        store.delete(TEST_ACCOUNT).expect("delete");
        store.delete(TEST_ACCOUNT).expect("delete again is a no-op");

        assert_eq!(store.get(TEST_ACCOUNT).expect("get"), None);
        assert_eq!(
            store.get(DEEPGRAM_API_KEY).expect("get").as_deref(),
            Some("api-key")
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_credentials_file_is_not_readable_by_other_users() {
        use std::os::unix::fs::PermissionsExt;

        let (store, path) = temp_store();
        store.set(TEST_ACCOUNT, "secret").expect("set");

        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "credentials file must be owner-only");
    }

    #[test]
    fn a_corrupt_credentials_file_reports_an_error_instead_of_pretending_it_is_empty() {
        let (store, path) = temp_store();
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, b"{not json").expect("write");

        assert!(matches!(
            store.get(TEST_ACCOUNT),
            Err(CredentialError::Corrupt { .. })
        ));
    }
}
