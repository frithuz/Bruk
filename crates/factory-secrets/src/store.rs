//! OS-native secret stores and in-memory test/demo store.
//!
//! - macOS: direct `Security.framework` bindings (`MacOsKeychainStore`) supporting custom
//!   `kSecAttrLabel`, prompt-free `status()` via `SecKeychainGetStatus`, atomic `SecItemUpdate`,
//!   and isolated ephemeral file keychains for automated tests.
//! - Linux & Windows: `keyring-core` with D-Bus Secret Service / Windows Credential Manager.
//! - Tests & Demo Mode: `MemoryStore` (never touches the OS keychain).

use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;
use zeroize::Zeroize;

/// Keychain service identifier (`kSecAttrService`) for Bruk's secrets.
pub const BRUK_KEYCHAIN_SERVICE: &str = "dev.frithuz.bruk.secrets";

/// Current availability of the underlying OS secret store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreStatus {
    Available,
    Locked,
    Unavailable(String),
}

impl StoreStatus {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    pub fn label(&self) -> String {
        match self {
            Self::Available => "Available".to_owned(),
            Self::Locked => "Locked".to_owned(),
            Self::Unavailable(msg) => format!("Unavailable: {msg}"),
        }
    }
}

/// Errors returned by [`SecretStore`] operations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SecretError {
    #[error("secret store is locked or access was cancelled")]
    Locked,
    #[error("secret store is unavailable: {0}")]
    Unavailable(String),
    #[error("secret store operation failed: {0}")]
    Platform(String),
    #[error("secret value is not valid UTF-8")]
    InvalidUtf8,
}

/// Trait implemented by OS-native secret stores and [`MemoryStore`].
pub trait SecretStore: Send + Sync {
    /// Checks store availability without prompting the user.
    fn status(&self) -> StoreStatus;
    /// Reads a secret by `<db_instance_id>:<secret_id>`. Returns `Ok(None)` when absent.
    fn get(&self, account_key: &str) -> Result<Option<SecretString>, SecretError>;
    /// Atomically creates or updates the secret value and human-readable label.
    fn set(&self, account_key: &str, label: &str, value: &SecretString) -> Result<(), SecretError>;
    /// Best-effort update of the human-readable item label (e.g. after rename).
    fn update_label(&self, account_key: &str, label: &str) -> Result<(), SecretError>;
    /// Deletes the secret item. Deleting a non-existent item counts as success (`Ok(())`).
    fn delete(&self, account_key: &str) -> Result<(), SecretError>;
    /// Whether this store is an ephemeral in-memory store (tests or Demo Mode).
    fn is_memory(&self) -> bool {
        false
    }
}

/// Returns the default OS-native [`SecretStore`] for the current platform.
pub fn default_secret_store() -> Arc<dyn SecretStore> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(MacOsKeychainStore::default_login())
    }
    #[cfg(target_os = "linux")]
    {
        Arc::new(KeyringCoreStore::new_linux())
    }
    #[cfg(target_os = "windows")]
    {
        Arc::new(KeyringCoreStore::new_windows())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Arc::new(UnavailableStore::new("unsupported platform"))
    }
}

// ---------------------------------------------------------------------------------------------
// MemoryStore (tests & Demo Mode)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct MemoryEntry {
    label: String,
    value: SecretString,
}

/// Thread-safe in-memory [`SecretStore`] used by tests and Demo Mode.
#[derive(Debug, Default)]
pub struct MemoryStore {
    entries: RwLock<BTreeMap<String, MemoryEntry>>,
    status: RwLock<Option<StoreStatus>>,
    fail_next_set: RwLock<Option<SecretError>>,
    fail_next_delete: RwLock<Option<SecretError>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_status(&self, status: StoreStatus) {
        *self.status.write().expect("memory store lock poisoned") = Some(status);
    }

    pub fn inject_next_set_error(&self, err: SecretError) {
        *self
            .fail_next_set
            .write()
            .expect("memory store lock poisoned") = Some(err);
    }

    pub fn inject_next_delete_error(&self, err: SecretError) {
        *self
            .fail_next_delete
            .write()
            .expect("memory store lock poisoned") = Some(err);
    }

    pub fn label_of(&self, account_key: &str) -> Option<String> {
        self.entries
            .read()
            .expect("memory store lock poisoned")
            .get(account_key)
            .map(|e| e.label.clone())
    }

    pub fn len(&self) -> usize {
        self.entries
            .read()
            .expect("memory store lock poisoned")
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn keys(&self) -> Vec<String> {
        self.entries
            .read()
            .expect("memory store lock poisoned")
            .keys()
            .cloned()
            .collect()
    }

    fn check_ready(&self) -> Result<(), SecretError> {
        match self.status() {
            StoreStatus::Available => Ok(()),
            StoreStatus::Locked => Err(SecretError::Locked),
            StoreStatus::Unavailable(msg) => Err(SecretError::Unavailable(msg)),
        }
    }
}

impl SecretStore for MemoryStore {
    fn status(&self) -> StoreStatus {
        self.status
            .read()
            .expect("memory store lock poisoned")
            .clone()
            .unwrap_or(StoreStatus::Available)
    }

    fn get(&self, account_key: &str) -> Result<Option<SecretString>, SecretError> {
        self.check_ready()?;
        Ok(self
            .entries
            .read()
            .expect("memory store lock poisoned")
            .get(account_key)
            .map(|entry| entry.value.clone()))
    }

    fn set(&self, account_key: &str, label: &str, value: &SecretString) -> Result<(), SecretError> {
        self.check_ready()?;
        if let Some(err) = self
            .fail_next_set
            .write()
            .expect("memory store lock poisoned")
            .take()
        {
            return Err(err);
        }
        self.entries
            .write()
            .expect("memory store lock poisoned")
            .insert(
                account_key.to_owned(),
                MemoryEntry {
                    label: label.to_owned(),
                    value: value.clone(),
                },
            );
        Ok(())
    }

    fn update_label(&self, account_key: &str, label: &str) -> Result<(), SecretError> {
        self.check_ready()?;
        if let Some(entry) = self
            .entries
            .write()
            .expect("memory store lock poisoned")
            .get_mut(account_key)
        {
            entry.label = label.to_owned();
        }
        Ok(())
    }

    fn delete(&self, account_key: &str) -> Result<(), SecretError> {
        self.check_ready()?;
        if let Some(err) = self
            .fail_next_delete
            .write()
            .expect("memory store lock poisoned")
            .take()
        {
            return Err(err);
        }
        self.entries
            .write()
            .expect("memory store lock poisoned")
            .remove(account_key);
        Ok(())
    }

    fn is_memory(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------------------------
// macOS Security.framework Store
// ---------------------------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod macos {
    use std::path::PathBuf;

    use core_foundation::{base::TCFType, data::CFData};
    use secrecy::{ExposeSecret, SecretString};
    use security_framework::{
        item::{
            ItemAddOptions, ItemAddValue, ItemClass, ItemSearchOptions, ItemUpdateOptions,
            ItemUpdateValue, Location, SearchResult, update_item,
        },
        os::macos::keychain::SecKeychain,
    };
    use security_framework_sys::base::{SecKeychainRef, errSecSuccess};
    use zeroize::Zeroize;

    use super::{BRUK_KEYCHAIN_SERVICE, SecretError, SecretStore, StoreStatus};

    // Standard macOS Security.framework status codes.
    const ERR_SEC_USER_CANCELED: i32 = -128;
    const ERR_SEC_AUTH_FAILED: i32 = -25293;
    const ERR_SEC_DUPLICATE_ITEM: i32 = -25299;
    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;
    const ERR_SEC_INTERACTION_NOT_ALLOWED: i32 = -25308;
    const ERR_SEC_NO_SUCH_KEYCHAIN: i32 = -25294;
    const K_SEC_UNLOCK_STATE_STATUS: u32 = 1 << 0;

    unsafe extern "C" {
        fn SecKeychainGetStatus(keychain: SecKeychainRef, keychain_status: *mut u32) -> i32;
    }

    /// macOS Keychain secret store backed directly by `Security.framework`.
    pub struct MacOsKeychainStore {
        service: String,
        /// Optional explicit keychain file path (used in automated tests with isolated keychains).
        keychain_path: Option<PathBuf>,
    }

    impl MacOsKeychainStore {
        /// Store targeting the user's default login keychain under [`BRUK_KEYCHAIN_SERVICE`].
        pub fn default_login() -> Self {
            Self {
                service: BRUK_KEYCHAIN_SERVICE.to_owned(),
                keychain_path: None,
            }
        }

        /// Store targeting a specific keychain file (for isolated integration tests).
        pub fn with_keychain_path(path: PathBuf) -> Self {
            Self {
                service: BRUK_KEYCHAIN_SERVICE.to_owned(),
                keychain_path: Some(path),
            }
        }

        fn open_keychain(&self) -> Result<SecKeychain, SecretError> {
            match &self.keychain_path {
                Some(path) => SecKeychain::open(path).map_err(map_sec_error),
                None => SecKeychain::default().map_err(map_sec_error),
            }
        }

        fn search_options(&self, kc: &SecKeychain, account_key: &str) -> ItemSearchOptions {
            let mut opts = ItemSearchOptions::new();
            opts.keychains(std::slice::from_ref(kc))
                .class(ItemClass::generic_password())
                .service(&self.service)
                .account(account_key);
            opts
        }
    }

    fn map_sec_error(err: security_framework::base::Error) -> SecretError {
        match err.code() {
            ERR_SEC_AUTH_FAILED | ERR_SEC_USER_CANCELED | ERR_SEC_INTERACTION_NOT_ALLOWED => {
                SecretError::Locked
            }
            ERR_SEC_NO_SUCH_KEYCHAIN => SecretError::Unavailable(err.to_string()),
            _ => SecretError::Platform(err.to_string()),
        }
    }

    impl SecretStore for MacOsKeychainStore {
        fn status(&self) -> StoreStatus {
            let kc = match self.open_keychain() {
                Ok(kc) => kc,
                Err(SecretError::Locked) => return StoreStatus::Locked,
                Err(err) => return StoreStatus::Unavailable(err.to_string()),
            };
            let mut flags: u32 = 0;
            let status = unsafe { SecKeychainGetStatus(kc.as_concrete_TypeRef(), &mut flags) };
            if status != errSecSuccess {
                return if status == ERR_SEC_AUTH_FAILED || status == ERR_SEC_INTERACTION_NOT_ALLOWED
                {
                    log::warn!("keychain.status locked code={status}");
                    StoreStatus::Locked
                } else {
                    log::warn!("keychain.status unavailable code={status}");
                    StoreStatus::Unavailable(format!("SecKeychainGetStatus failed ({status})"))
                };
            }
            if (flags & K_SEC_UNLOCK_STATE_STATUS) != 0 {
                StoreStatus::Available
            } else {
                log::warn!("keychain.status locked flags={flags}");
                StoreStatus::Locked
            }
        }

        fn get(&self, account_key: &str) -> Result<Option<SecretString>, SecretError> {
            let kc = self.open_keychain()?;
            let mut search = self.search_options(&kc, account_key);
            search.load_data(true).limit(1);
            match search.search() {
                Ok(results) => {
                    for result in results {
                        if let SearchResult::Data(mut bytes) = result {
                            let utf8 = String::from_utf8(bytes.clone());
                            bytes.zeroize();
                            let s = utf8.map_err(|_| SecretError::InvalidUtf8)?;
                            return Ok(Some(SecretString::from(s)));
                        }
                    }
                    Ok(None)
                }
                Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
                Err(err) => Err(map_sec_error(err)),
            }
        }

        fn set(
            &self,
            account_key: &str,
            label: &str,
            value: &SecretString,
        ) -> Result<(), SecretError> {
            let kc = self.open_keychain()?;
            let bytes = value.expose_secret().as_bytes();

            // Try atomic in-place update first so an existing working secret is never deleted
            // before the replacement succeeds.
            let search = self.search_options(&kc, account_key);
            let mut update = ItemUpdateOptions::new();
            update.set_value(ItemUpdateValue::Data(CFData::from_buffer(bytes)));
            update.set_label(label);
            match update_item(&search, &update) {
                Ok(()) => return Ok(()),
                Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => {}
                Err(err) => return Err(map_sec_error(err)),
            }

            // Item doesn't exist yet: add it.
            let mut add = ItemAddOptions::new(ItemAddValue::Data {
                class: ItemClass::generic_password(),
                data: CFData::from_buffer(bytes),
            });
            add.set_service(&self.service)
                .set_account_name(account_key)
                .set_label(label)
                .set_location(Location::FileKeychain(kc.clone()));
            match add.add() {
                Ok(()) => Ok(()),
                Err(err) if err.code() == ERR_SEC_DUPLICATE_ITEM => {
                    // Concurrent add: retry update once.
                    update_item(&search, &update).map_err(map_sec_error)
                }
                Err(err) => Err(map_sec_error(err)),
            }
        }

        fn update_label(&self, account_key: &str, label: &str) -> Result<(), SecretError> {
            let kc = self.open_keychain()?;
            let search = self.search_options(&kc, account_key);
            let mut update = ItemUpdateOptions::new();
            update.set_label(label);
            match update_item(&search, &update) {
                Ok(()) => Ok(()),
                Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
                Err(err) => Err(map_sec_error(err)),
            }
        }

        fn delete(&self, account_key: &str) -> Result<(), SecretError> {
            let kc = self.open_keychain()?;
            let search = self.search_options(&kc, account_key);
            match search.delete() {
                Ok(()) => Ok(()),
                Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
                Err(err) => Err(map_sec_error(err)),
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::MacOsKeychainStore;

// ---------------------------------------------------------------------------------------------
// Linux / Windows KeyringCoreStore
// ---------------------------------------------------------------------------------------------

#[cfg(any(target_os = "linux", target_os = "windows"))]
mod keyring_store {
    use std::sync::Arc;

    use keyring_core::api::CredentialStoreApi;
    use secrecy::{ExposeSecret, SecretString};

    use super::{BRUK_KEYCHAIN_SERVICE, SecretError, SecretStore, StoreStatus};

    pub struct KeyringCoreStore {
        store: Result<Arc<dyn CredentialStoreApi + Send + Sync>, String>,
    }

    impl KeyringCoreStore {
        #[cfg(target_os = "linux")]
        pub fn new_linux() -> Self {
            let store = dbus_secret_service_keyring_store::Store::new()
                .map(|s| s as Arc<dyn CredentialStoreApi + Send + Sync>)
                .map_err(|e| e.to_string());
            Self { store }
        }

        #[cfg(target_os = "windows")]
        pub fn new_windows() -> Self {
            let store = windows_native_keyring_store::Store::new()
                .map(|s| s as Arc<dyn CredentialStoreApi + Send + Sync>)
                .map_err(|e| e.to_string());
            Self { store }
        }
    }

    impl SecretStore for KeyringCoreStore {
        fn status(&self) -> StoreStatus {
            match &self.store {
                Ok(_) => StoreStatus::Available,
                Err(msg) => StoreStatus::Unavailable(msg.clone()),
            }
        }

        fn get(&self, account_key: &str) -> Result<Option<SecretString>, SecretError> {
            let store = self
                .store
                .as_ref()
                .map_err(|m| SecretError::Unavailable(m.clone()))?;
            let entry = store
                .build(BRUK_KEYCHAIN_SERVICE, account_key, None)
                .map_err(|e| SecretError::Platform(e.to_string()))?;
            match entry.get_password() {
                Ok(val) => Ok(Some(SecretString::from(val))),
                Err(keyring_core::Error::NoEntry) => Ok(None),
                Err(err) => Err(SecretError::Platform(err.to_string())),
            }
        }

        fn set(
            &self,
            account_key: &str,
            _label: &str,
            value: &SecretString,
        ) -> Result<(), SecretError> {
            let store = self
                .store
                .as_ref()
                .map_err(|m| SecretError::Unavailable(m.clone()))?;
            let entry = store
                .build(BRUK_KEYCHAIN_SERVICE, account_key, None)
                .map_err(|e| SecretError::Platform(e.to_string()))?;
            entry
                .set_password(value.expose_secret())
                .map_err(|e| SecretError::Platform(e.to_string()))
        }

        fn update_label(&self, _account_key: &str, _label: &str) -> Result<(), SecretError> {
            Ok(())
        }

        fn delete(&self, account_key: &str) -> Result<(), SecretError> {
            let store = self
                .store
                .as_ref()
                .map_err(|m| SecretError::Unavailable(m.clone()))?;
            let entry = store
                .build(BRUK_KEYCHAIN_SERVICE, account_key, None)
                .map_err(|e| SecretError::Platform(e.to_string()))?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
                Err(err) => Err(SecretError::Platform(err.to_string())),
            }
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub use keyring_store::KeyringCoreStore;

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
struct UnavailableStore {
    reason: String,
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
impl UnavailableStore {
    fn new(reason: &str) -> Self {
        Self {
            reason: reason.to_owned(),
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
impl SecretStore for UnavailableStore {
    fn status(&self) -> StoreStatus {
        StoreStatus::Unavailable(self.reason.clone())
    }
    fn get(&self, _: &str) -> Result<Option<SecretString>, SecretError> {
        Err(SecretError::Unavailable(self.reason.clone()))
    }
    fn set(&self, _: &str, _: &str, _: &SecretString) -> Result<(), SecretError> {
        Err(SecretError::Unavailable(self.reason.clone()))
    }
    fn update_label(&self, _: &str, _: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable(self.reason.clone()))
    }
    fn delete(&self, _: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable(self.reason.clone()))
    }
}

/// Helper to read back and verify exact byte equality after a Keychain write.
pub fn write_and_verify(
    store: &dyn SecretStore,
    account_key: &str,
    label: &str,
    value: &SecretString,
) -> Result<(), SecretError> {
    store.set(account_key, label, value)?;
    let read_back = store.get(account_key)?;
    match read_back {
        Some(actual) if actual.expose_secret().as_bytes() == value.expose_secret().as_bytes() => {
            Ok(())
        }
        Some(_) => Err(SecretError::Platform(
            "verification failed: stored secret did not match written bytes".to_owned(),
        )),
        None => Err(SecretError::Platform(
            "verification failed: secret disappeared immediately after write".to_owned(),
        )),
    }
}

/// Zeroizes a mutable string buffer in place.
pub fn zeroize_string(buf: &mut String) {
    buf.zeroize();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_roundtrip_and_status() {
        let store = MemoryStore::new();
        assert_eq!(store.status(), StoreStatus::Available);
        let secret = SecretString::from("super-secret-value".to_owned());
        write_and_verify(&store, "inst:1", "Bruk: TEST", &secret).unwrap();
        assert_eq!(
            store.get("inst:1").unwrap().unwrap().expose_secret(),
            "super-secret-value"
        );
        store.update_label("inst:1", "Bruk: RENAMED").unwrap();
        assert_eq!(store.label_of("inst:1").as_deref(), Some("Bruk: RENAMED"));
        store.delete("inst:1").unwrap();
        assert!(store.get("inst:1").unwrap().is_none());
        // Deleting missing item is a no-op.
        store.delete("inst:missing").unwrap();

        store.set_status(StoreStatus::Locked);
        assert_eq!(store.get("inst:1").unwrap_err(), SecretError::Locked);
    }

    /// Automated macOS Keychain integration test using an isolated ephemeral keychain in TempDir.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_keychain_ephemeral_integration() {
        use std::process::Command;
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let kc_path = temp.path().join("bruk-test.keychain-db");
        let password = format!("pw-{}", uuid::Uuid::new_v4());

        let create = Command::new("security")
            .args(["create-keychain", "-p", &password])
            .arg(&kc_path)
            .status()
            .expect("security create-keychain");
        assert!(create.success(), "create-keychain failed");

        let unlock = Command::new("security")
            .args(["unlock-keychain", "-p", &password])
            .arg(&kc_path)
            .status()
            .expect("security unlock-keychain");
        assert!(unlock.success(), "unlock-keychain failed");

        let store = MacOsKeychainStore::with_keychain_path(kc_path.clone());
        assert_eq!(store.status(), StoreStatus::Available);

        // Create & read (including multi-line UTF-8)
        let val1 = SecretString::from("line-1\nline-2-🔑-utf8\n".to_owned());
        write_and_verify(&store, "dbA:sec1", "Bruk: TEST_KEY (api)", &val1).unwrap();
        let got = store.get("dbA:sec1").unwrap().unwrap();
        assert_eq!(got.expose_secret(), val1.expose_secret());

        // Verify db_instance_id isolation
        assert!(store.get("dbB:sec1").unwrap().is_none());

        // Atomic in-place update
        let val2 = SecretString::from("updated-secret-value-9999".to_owned());
        write_and_verify(&store, "dbA:sec1", "Bruk: TEST_KEY (api)", &val2).unwrap();
        assert_eq!(
            store.get("dbA:sec1").unwrap().unwrap().expose_secret(),
            val2.expose_secret()
        );

        // Label rename update
        store
            .update_label("dbA:sec1", "Bruk: RENAMED_KEY (api)")
            .unwrap();

        // Delete & idempotent delete
        store.delete("dbA:sec1").unwrap();
        assert!(store.get("dbA:sec1").unwrap().is_none());
        store.delete("dbA:sec1").unwrap();

        // Lock detection without prompting
        let lock = Command::new("security")
            .arg("lock-keychain")
            .arg(&kc_path)
            .status()
            .expect("security lock-keychain");
        assert!(lock.success(), "lock-keychain failed");
        assert_eq!(store.status(), StoreStatus::Locked);

        // Clean up ephemeral keychain
        let _ = Command::new("security")
            .arg("delete-keychain")
            .arg(&kc_path)
            .status();
    }
}
