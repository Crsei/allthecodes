//! API key validation and storage
//!
//! Supports:
//! - Format validation (sk-ant-* prefix)
//! - System keychain storage
//! - Environment variable (`ANTHROPIC_API_KEY`)

use anyhow::Result;

/// Service name used for allthecodes API keys in the system keychain.
pub const KEYCHAIN_SERVICE_NAME: &str = "allthecodes";
const LEGACY_KEYCHAIN_SERVICE_NAME: &str = "cc-rust";

/// Account name used for the Anthropic API key in the system keychain.
pub const KEYCHAIN_ACCOUNT_API_KEY: &str = "api-key";

/// Account name used for the OpenAI Platform API key in the system keychain.
pub const KEYCHAIN_ACCOUNT_OPENAI_API_KEY: &str = "openai-api-key";

/// Validate an API key format.
///
/// Valid keys start with `sk-ant-` and are longer than 20 characters.
pub fn validate_api_key(key: &str) -> bool {
    key.starts_with("sk-ant-") && key.len() > 20
}

/// Validate an OpenAI API key format without binding to one current prefix.
///
/// OpenAI has used several prefixes over time. For persisted keys we only
/// require a trimmed, non-empty secret with enough entropy-looking length.
pub fn validate_openai_api_key(key: &str) -> bool {
    let trimmed = key.trim();
    trimmed.len() >= 20 && !trimmed.chars().any(char::is_whitespace)
}

/// Store API key to the system keychain.
pub fn store_api_key(key: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE_NAME, KEYCHAIN_ACCOUNT_API_KEY)?;
    entry.set_password(key)?;
    Ok(())
}

/// Store an OpenAI API key to the system keychain.
pub fn store_openai_api_key(key: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE_NAME, KEYCHAIN_ACCOUNT_OPENAI_API_KEY)?;
    entry.set_password(key.trim())?;
    Ok(())
}

/// Load API key from the system keychain.
///
/// Returns `Ok(None)` if no key is stored.
pub fn load_api_key() -> Result<Option<String>> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE_NAME, KEYCHAIN_ACCOUNT_API_KEY)?;
    match entry.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => load_legacy_keychain_entry(KEYCHAIN_ACCOUNT_API_KEY),
        Err(e) => Err(e.into()),
    }
}

/// Load OpenAI API key from the system keychain.
///
/// Returns `Ok(None)` if no key is stored.
pub fn load_openai_api_key() -> Result<Option<String>> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE_NAME, KEYCHAIN_ACCOUNT_OPENAI_API_KEY)?;
    match entry.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => load_legacy_keychain_entry(KEYCHAIN_ACCOUNT_OPENAI_API_KEY),
        Err(e) => Err(e.into()),
    }
}

fn load_legacy_keychain_entry(account: &str) -> Result<Option<String>> {
    let legacy_entry = keyring::Entry::new(LEGACY_KEYCHAIN_SERVICE_NAME, account)?;
    match legacy_entry.get_password() {
        Ok(key) => {
            let new_entry = keyring::Entry::new(KEYCHAIN_SERVICE_NAME, account)?;
            if let Err(err) = new_entry.set_password(&key) {
                tracing::warn!(
                    account,
                    error = %err,
                    "failed to migrate legacy cc-rust keychain entry to allthecodes"
                );
            }
            Ok(Some(key))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Remove API key from the system keychain.
///
/// Used by the `/logout` command.
pub fn remove_api_key() -> Result<()> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE_NAME, KEYCHAIN_ACCOUNT_API_KEY)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()), // already gone
        Err(e) => Err(e.into()),
    }
}

/// Remove OpenAI API key from the system keychain.
pub fn remove_openai_api_key() -> Result<()> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE_NAME, KEYCHAIN_ACCOUNT_OPENAI_API_KEY)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    static KEYRING_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static TEST_KEYCHAIN: OnceLock<Mutex<HashMap<(String, String), Vec<u8>>>> = OnceLock::new();

    #[derive(Debug)]
    struct PersistentTestCredential {
        service: String,
        user: String,
    }

    impl keyring::credential::CredentialApi for PersistentTestCredential {
        fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
            TEST_KEYCHAIN
                .get_or_init(Default::default)
                .lock()
                .expect("test keychain poisoned")
                .insert((self.service.clone(), self.user.clone()), secret.to_vec());
            Ok(())
        }

        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            TEST_KEYCHAIN
                .get_or_init(Default::default)
                .lock()
                .expect("test keychain poisoned")
                .get(&(self.service.clone(), self.user.clone()))
                .cloned()
                .ok_or(keyring::Error::NoEntry)
        }

        fn delete_credential(&self) -> keyring::Result<()> {
            TEST_KEYCHAIN
                .get_or_init(Default::default)
                .lock()
                .expect("test keychain poisoned")
                .remove(&(self.service.clone(), self.user.clone()))
                .map(|_| ())
                .ok_or(keyring::Error::NoEntry)
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    struct PersistentTestCredentialBuilder;

    impl keyring::credential::CredentialBuilderApi for PersistentTestCredentialBuilder {
        fn build(
            &self,
            _target: Option<&str>,
            service: &str,
            user: &str,
        ) -> keyring::Result<Box<keyring::Credential>> {
            Ok(Box::new(PersistentTestCredential {
                service: service.to_string(),
                user: user.to_string(),
            }))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn persistence(&self) -> keyring::credential::CredentialPersistence {
            keyring::credential::CredentialPersistence::ProcessOnly
        }
    }

    fn use_persistent_test_keyring() {
        TEST_KEYCHAIN
            .get_or_init(Default::default)
            .lock()
            .expect("test keychain poisoned")
            .clear();
        keyring::set_default_credential_builder(Box::new(PersistentTestCredentialBuilder));
    }

    #[test]
    fn test_valid_api_key() {
        assert!(validate_api_key("sk-ant-api03-abcdefghijklmnop"));
        assert!(validate_api_key("sk-ant-xxxxxxxxxxxxxxxxxxxx1"));
    }

    #[test]
    fn test_invalid_api_key() {
        assert!(!validate_api_key("sk-ant-short"));
        assert!(!validate_api_key("wrong-prefix-abcdefghijklmnop"));
        assert!(!validate_api_key(""));
    }

    #[test]
    fn test_openai_api_key_validation_is_provider_prefix_tolerant() {
        assert!(validate_openai_api_key("sk-proj-abcdefghijklmnopqrst"));
        assert!(validate_openai_api_key("oaikey-live-abcdefghijklmnop"));
        assert!(validate_openai_api_key("  sk-abcdefghijklmnopqrst  "));
        assert!(!validate_openai_api_key(""));
        assert!(!validate_openai_api_key("short"));
        assert!(!validate_openai_api_key("sk-proj-has whitespace"));
    }

    #[test]
    fn keychain_service_name_is_path_isolated() {
        assert_eq!(KEYCHAIN_SERVICE_NAME, "allthecodes");
        assert_ne!(KEYCHAIN_SERVICE_NAME, "Codex");
        assert_ne!(KEYCHAIN_SERVICE_NAME, "Claude");
    }

    #[test]
    fn keychain_accounts_are_provider_scoped() {
        assert_eq!(KEYCHAIN_ACCOUNT_API_KEY, "api-key");
        assert_eq!(KEYCHAIN_ACCOUNT_OPENAI_API_KEY, "openai-api-key");
        assert_ne!(KEYCHAIN_ACCOUNT_API_KEY, KEYCHAIN_ACCOUNT_OPENAI_API_KEY);
    }

    #[test]
    fn provider_scoped_keychain_entries_do_not_overlap() {
        let _guard = KEYRING_TEST_LOCK
            .lock()
            .expect("keyring test lock poisoned");
        use_persistent_test_keyring();

        remove_api_key().unwrap();
        remove_openai_api_key().unwrap();

        store_api_key("sk-ant-api03-abcdefghijklmnop").unwrap();
        store_openai_api_key("sk-proj-abcdefghijklmnopqrst").unwrap();

        assert_eq!(
            load_api_key().unwrap().as_deref(),
            Some("sk-ant-api03-abcdefghijklmnop")
        );
        assert_eq!(
            load_openai_api_key().unwrap().as_deref(),
            Some("sk-proj-abcdefghijklmnopqrst")
        );

        remove_api_key().unwrap();
        assert_eq!(load_api_key().unwrap(), None);
        assert_eq!(
            load_openai_api_key().unwrap().as_deref(),
            Some("sk-proj-abcdefghijklmnopqrst")
        );

        remove_openai_api_key().unwrap();
        assert_eq!(load_openai_api_key().unwrap(), None);
    }
}
