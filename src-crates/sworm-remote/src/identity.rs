use crate::client::RemoteError;
use rustls_pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, ErrorKind, Write},
    path::{Component, Path, PathBuf},
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const LOCK_WAIT: Duration = Duration::from_secs(10);
const LOCK_POLL: Duration = Duration::from_millis(10);
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// SHA-256 digest of a certificate's DER encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub [u8; 32]);

impl Fingerprint {
    pub fn of_cert(cert: &CertificateDer<'_>) -> Self {
        Self(Sha256::digest(cert.as_ref()).into())
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "SHA256:{}", hex::encode(self.0))
    }
}

impl FromStr for Fingerprint {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let hex = value
            .get(..7)
            .filter(|prefix| prefix.eq_ignore_ascii_case("SHA256:"))
            .map_or(value, |_| &value[7..]);
        let mut bytes = [0; 32];
        hex::decode_to_slice(hex, &mut bytes)
            .map_err(|error| format!("invalid SHA-256 fingerprint: {error}"))?;
        Ok(Self(bytes))
    }
}

/// Certificate and private key used for mutual TLS.
pub struct Identity {
    pub cert: CertificateDer<'static>,
    pub key: PrivateKeyDer<'static>,
}

impl Identity {
    /// Load a persisted identity, or atomically create one when absent.
    pub fn load_or_generate(dir: &Path, stem: &str) -> Result<Self, RemoteError> {
        validate_stem(stem)?;
        secure_directory(dir)?;

        let identity_path = dir.join(format!("{stem}.pem"));
        let load = || {
            if entry_exists(&identity_path)? {
                load_existing(&identity_path).map(Some)
            } else {
                Ok(None)
            }
        };
        if let Some(identity) = load()? {
            return Ok(identity);
        }

        let _lock = GenerationLock::acquire(&dir.join(format!(".{stem}.identity.lock")))?;
        if let Some(identity) = load()? {
            return Ok(identity);
        }
        generate(dir, stem, &identity_path)
    }

    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::of_cert(&self.cert)
    }
}

fn validate_stem(stem: &str) -> Result<(), RemoteError> {
    let mut components = Path::new(stem).components();
    if matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none() {
        Ok(())
    } else {
        Err(RemoteError::Identity(format!(
            "identity stem must be one path component: {stem:?}"
        )))
    }
}

fn generate(dir: &Path, stem: &str, identity_path: &Path) -> Result<Identity, RemoteError> {
    let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519)
        .map_err(|error| identity_error("generate Ed25519 key", error))?;
    let certificate = rcgen::CertificateParams::new(vec!["sworm".to_string()])
        .and_then(|params| params.self_signed(&key_pair))
        .map_err(|error| identity_error("generate self-signed certificate", error))?;
    let contents = format!("{}{}", certificate.pem(), key_pair.serialize_pem());

    let mut temporary = TemporaryFiles(Vec::new());
    let identity_temp = write_temp(dir, stem, "pem", contents.as_bytes(), 0o600)?;
    temporary.0.push(identity_temp.clone());
    publish(&identity_temp, identity_path, 0o600)?;
    sync_directory(dir)?;
    drop(temporary);

    load_existing(identity_path)
}

fn load_existing(identity_path: &Path) -> Result<Identity, RemoteError> {
    require_regular_file(identity_path)?;
    set_mode(identity_path, 0o600)?;

    let cert = CertificateDer::from_pem_file(identity_path).map_err(|error| {
        RemoteError::Identity(format!(
            "cannot read identity certificate {}: {error}",
            identity_path.display()
        ))
    })?;
    let key = PrivateKeyDer::from_pem_file(identity_path).map_err(|error| {
        RemoteError::Identity(format!(
            "cannot read identity key {}: {error}",
            identity_path.display()
        ))
    })?;

    rustls::sign::CertifiedKey::from_der(
        vec![cert.clone()],
        key.clone_key(),
        &rustls::crypto::ring::default_provider(),
    )
    .map_err(|error| {
        RemoteError::Identity(format!(
            "identity certificate and key do not form a usable pair: {error}"
        ))
    })?;

    Ok(Identity { cert, key })
}

fn entry_exists(path: &Path) -> Result<bool, RemoteError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(RemoteError::Identity(format!(
            "cannot inspect identity path {}: {error}",
            path.display()
        ))),
    }
}
fn require_regular_file(path: &Path) -> Result<(), RemoteError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        RemoteError::Identity(format!(
            "cannot inspect identity file {}: {error}",
            path.display()
        ))
    })?;
    if metadata.file_type().is_file() {
        Ok(())
    } else {
        Err(RemoteError::Identity(format!(
            "identity path {} is not a regular file",
            path.display()
        )))
    }
}

fn secure_directory(dir: &Path) -> Result<(), RemoteError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(dir).map_err(|error| {
            RemoteError::Identity(format!(
                "cannot create identity directory {}: {error}",
                dir.display()
            ))
        })?;
        set_mode(dir, 0o700)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir).map_err(|error| {
            RemoteError::Identity(format!(
                "cannot create identity directory {}: {error}",
                dir.display()
            ))
        })
    }
}

fn write_temp(
    dir: &Path,
    stem: &str,
    kind: &str,
    contents: &[u8],
    mode: u32,
) -> Result<PathBuf, RemoteError> {
    for _ in 0..128 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!(
            ".{stem}.{kind}.tmp-{}-{sequence}",
            std::process::id()
        ));
        let mut file = match open_new(&path, mode) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(RemoteError::Identity(format!(
                    "cannot create temporary identity file {}: {error}",
                    path.display()
                )))
            }
        };
        if let Err(error) = file.write_all(contents).and_then(|()| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(RemoteError::Identity(format!(
                "cannot write temporary identity file {}: {error}",
                path.display()
            )));
        }
        return Ok(path);
    }
    Err(RemoteError::Identity(
        "cannot allocate a temporary identity filename".to_string(),
    ))
}

fn publish(source: &Path, destination: &Path, mode: u32) -> Result<(), RemoteError> {
    fs::hard_link(source, destination).map_err(|error| {
        RemoteError::Identity(format!(
            "cannot create identity file {} without overwriting existing data: {error}",
            destination.display()
        ))
    })?;
    set_mode(destination, mode)
}

fn open_new(path: &Path, mode: u32) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    options.open(path)
}

fn set_mode(path: &Path, mode: u32) -> Result<(), RemoteError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|error| {
            RemoteError::Identity(format!(
                "cannot secure identity path {}: {error}",
                path.display()
            ))
        })
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

fn sync_directory(dir: &Path) -> Result<(), RemoteError> {
    #[cfg(unix)]
    {
        File::open(dir)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                RemoteError::Identity(format!(
                    "cannot persist identity directory {}: {error}",
                    dir.display()
                ))
            })
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

fn identity_error(context: &str, error: impl fmt::Display) -> RemoteError {
    RemoteError::Identity(format!("{context}: {error}"))
}

struct GenerationLock {
    _file: File,
}

impl GenerationLock {
    fn acquire(path: &Path) -> Result<Self, RemoteError> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path).map_err(|error| {
            RemoteError::Identity(format!(
                "cannot open identity lock {}: {error}",
                path.display()
            ))
        })?;
        let started = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(std::fs::TryLockError::WouldBlock) => {
                    if started.elapsed() >= LOCK_WAIT {
                        return Err(RemoteError::Identity(format!(
                            "timed out waiting for identity lock {}",
                            path.display()
                        )));
                    }
                    thread::sleep(LOCK_POLL);
                }
                Err(std::fs::TryLockError::Error(error)) => {
                    return Err(RemoteError::Identity(format!(
                        "cannot acquire identity lock {}: {error}",
                        path.display()
                    )))
                }
            }
        }
    }
}

struct TemporaryFiles(Vec<PathBuf>);

impl Drop for TemporaryFiles {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn fingerprint_display_parse_round_trip() {
        let fingerprint = Fingerprint([0xab; 32]);
        assert_eq!(
            fingerprint.to_string(),
            "SHA256:abababababababababababababababababababababababababababababababab"
        );
        assert_eq!(fingerprint.to_string().parse(), Ok(fingerprint));
        assert_eq!(
            format!("sha256:{}", hex::encode_upper(fingerprint.0)).parse(),
            Ok(fingerprint)
        );
        assert_eq!(
            "abababababababababababababababababababababababababababababababab".parse(),
            Ok(fingerprint)
        );
    }

    #[test]
    fn fingerprint_rejects_bad_input() {
        for value in [
            "",
            "SHA256:",
            "00",
            "SHA256:abababababababababababababababababababababababababababababababaz",
            "SHA256:abababababababababababababababababababababababababababababababab00",
        ] {
            assert!(value.parse::<Fingerprint>().is_err(), "accepted {value:?}");
        }
    }

    #[test]
    fn identity_is_stable_and_secure() {
        let directory = tempfile::tempdir().unwrap();
        let first = Identity::load_or_generate(directory.path(), "client").unwrap();
        let second = Identity::load_or_generate(directory.path(), "client").unwrap();
        assert_eq!(first.fingerprint(), second.fingerprint());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(directory.path().join("client.pem"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn corrupt_identity_is_never_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let identity_path = directory.path().join("client.pem");
        fs::write(&identity_path, b"keep this identity").unwrap();
        assert!(Identity::load_or_generate(directory.path(), "client").is_err());
        assert_eq!(fs::read(identity_path).unwrap(), b"keep this identity");
    }

    #[test]
    fn mismatched_certificate_and_key_are_rejected_without_replacement() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        Identity::load_or_generate(first.path(), "client").unwrap();
        Identity::load_or_generate(second.path(), "client").unwrap();

        let first_pem = fs::read_to_string(first.path().join("client.pem")).unwrap();
        let second_pem = fs::read_to_string(second.path().join("client.pem")).unwrap();
        let key_marker = "-----BEGIN PRIVATE KEY-----";
        let mismatched = format!(
            "{}{}",
            &first_pem[..first_pem.find(key_marker).unwrap()],
            &second_pem[second_pem.find(key_marker).unwrap()..]
        );
        let identity_path = second.path().join("client.pem");
        fs::write(&identity_path, mismatched).unwrap();
        let before = fs::read(&identity_path).unwrap();

        assert!(Identity::load_or_generate(second.path(), "client").is_err());
        assert_eq!(fs::read(identity_path).unwrap(), before);
    }

    #[test]
    fn concurrent_generation_keeps_one_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = Arc::new(directory.path().to_owned());
        let barrier = Arc::new(Barrier::new(8));
        let threads = (0..8)
            .map(|_| {
                let path = Arc::clone(&path);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    Identity::load_or_generate(&path, "client")
                        .unwrap()
                        .fingerprint()
                })
            })
            .collect::<Vec<_>>();
        let fingerprints = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        assert!(fingerprints.iter().all(|value| *value == fingerprints[0]));
    }

    #[test]
    fn stale_lock_file_does_not_block_generation() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join(".client.identity.lock"), b"stale").unwrap();
        Identity::load_or_generate(directory.path(), "client").unwrap();
    }
}
