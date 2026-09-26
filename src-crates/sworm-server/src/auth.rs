use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    str::FromStr,
};
use sworm_remote::Fingerprint;
use uuid::Uuid;

const AUTHORIZED_KEYS: &str = "authorized_keys";
const PAIRING_TOKEN: &str = "pairing-token";
const TOKEN_LIFETIME_SECONDS: i64 = 10 * 60;

#[derive(Serialize, Deserialize)]
struct PendingToken {
    token: String,
    expires_at: i64,
}

/// `extra` is an operator-managed key file (e.g. from the NixOS module) that is
/// read-only to the server; pairing only ever appends to the config-dir file.
pub fn is_authorized(config_dir: &Path, fingerprint: Fingerprint, extra: Option<&Path>) -> bool {
    let primary = config_dir.join(AUTHORIZED_KEYS);
    file_contains_fingerprint(&primary, fingerprint)
        || extra.is_some_and(|path| file_contains_fingerprint(path, fingerprint))
}

fn file_contains_fingerprint(path: &Path, fingerprint: Fingerprint) -> bool {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return false,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "failed to read authorized keys file");
            return false;
        }
    };

    contents.lines().any(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return false;
        }
        let Some(value) = line.split_whitespace().next() else {
            return false;
        };
        match Fingerprint::from_str(value) {
            Ok(candidate) => candidate == fingerprint,
            Err(error) => {
                tracing::warn!(fingerprint = value, %error, "skipping invalid authorized key");
                false
            }
        }
    })
}

pub fn append_authorized(
    config_dir: &Path,
    fingerprint: Fingerprint,
    name: &str,
) -> io::Result<()> {
    fs::create_dir_all(config_dir)?;
    let name = name.split_whitespace().collect::<Vec<_>>().join("-");
    let name = if name.is_empty() { "unnamed" } else { &name };
    let path = config_dir.join(AUTHORIZED_KEYS);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    writeln!(file, "{fingerprint} {name}")?;
    file.sync_data()
}

pub fn write_pairing_token(config_dir: &Path) -> io::Result<String> {
    fs::create_dir_all(config_dir)?;
    let token = Uuid::new_v4().simple().to_string();
    let pending = PendingToken {
        token: token.clone(),
        expires_at: Utc::now().timestamp() + TOKEN_LIFETIME_SECONDS,
    };
    let contents = serde_json::to_vec(&pending).map_err(io::Error::other)?;
    let path = config_dir.join(PAIRING_TOKEN);
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    file.write_all(&contents)?;
    file.sync_all()?;
    Ok(token)
}

pub(crate) fn consume_pairing_token(config_dir: &Path, presented: &str) -> io::Result<bool> {
    let path = config_dir.join(PAIRING_TOKEN);
    let contents = match fs::read(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let pending: PendingToken = match serde_json::from_slice(&contents) {
        Ok(pending) => pending,
        Err(_) => return Ok(false),
    };
    if Utc::now().timestamp() >= pending.expires_at || presented != pending.token {
        return Ok(false);
    }
    fs::remove_file(path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingerprint(byte: u8) -> Fingerprint {
        Fingerprint([byte; 32])
    }

    #[test]
    fn paired_and_operator_keys_both_authorize() {
        let config_dir = tempfile::tempdir().unwrap();
        let extra = config_dir.path().join("operator-keys");
        let (paired, declared, stranger) = (fingerprint(1), fingerprint(2), fingerprint(3));
        append_authorized(config_dir.path(), paired, "laptop").unwrap();
        fs::write(&extra, format!("# nix\n{declared} desktop\n")).unwrap();

        assert!(is_authorized(config_dir.path(), paired, Some(&extra)));
        assert!(is_authorized(config_dir.path(), declared, Some(&extra)));
        assert!(!is_authorized(config_dir.path(), declared, None));
        assert!(!is_authorized(config_dir.path(), stranger, Some(&extra)));
        assert!(is_authorized(
            config_dir.path(),
            paired,
            Some(&config_dir.path().join("missing"))
        ));
        assert!(!is_authorized(
            config_dir.path(),
            stranger,
            Some(&config_dir.path().join("missing"))
        ));
    }
}
