use tracing::info;

/// Credential service backed by the Linux keyring (Secret Service).
///
/// Secrets live in the OS keyring, not in SQLite. The database stores
/// only references (service + account) so the app knows what exists.
pub struct CredentialService;

const SERVICE_NAME: &str = "dev.ade";

impl CredentialService {
    pub fn new() -> Self {
        Self
    }

    /// Write a test secret, read it back, delete it, and return a status report.
    pub fn smoke_test(&self) -> Result<String, String> {
        let account = "smoke-test";
        let test_value = "ade-keyring-test-value";

        // Attempt to create a keyring entry
        let entry = keyring::Entry::new(SERVICE_NAME, account).map_err(|e| {
            format!(
                "Failed to create keyring entry (Secret Service may be unavailable): {}",
                e
            )
        })?;

        // Write
        entry
            .set_password(test_value)
            .map_err(|e| format!("Keyring write failed: {}", e))?;
        info!("Keyring smoke test: write OK");

        // Read back
        let readback = entry
            .get_password()
            .map_err(|e| format!("Keyring read failed: {}", e))?;

        if readback != test_value {
            return Err(format!(
                "Keyring readback mismatch: expected '{}', got '{}'",
                test_value, readback
            ));
        }
        info!("Keyring smoke test: read OK");

        // Delete
        entry
            .delete_credential()
            .map_err(|e| format!("Keyring delete failed: {}", e))?;
        info!("Keyring smoke test: delete OK");

        Ok("Keyring OK: write/read/delete succeeded".to_string())
    }

    /// Store a secret in the keyring.
    #[allow(dead_code)]
    pub fn set_secret(&self, account: &str, secret: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE_NAME, account)
            .map_err(|e| format!("Failed to create keyring entry: {}", e))?;
        entry
            .set_password(secret)
            .map_err(|e| format!("Failed to set secret: {}", e))?;
        Ok(())
    }

    /// Retrieve a secret from the keyring.
    #[allow(dead_code)]
    pub fn get_secret(&self, account: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(SERVICE_NAME, account)
            .map_err(|e| format!("Failed to create keyring entry: {}", e))?;
        match entry.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("Failed to get secret: {}", e)),
        }
    }

    /// Delete a secret from the keyring.
    #[allow(dead_code)]
    pub fn delete_secret(&self, account: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE_NAME, account)
            .map_err(|e| format!("Failed to create keyring entry: {}", e))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // already gone
            Err(e) => Err(format!("Failed to delete secret: {}", e)),
        }
    }
}
