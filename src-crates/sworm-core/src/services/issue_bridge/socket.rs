//! Socket path resolution + atomic 0o700 directory creation. Pure
//! filesystem helpers, no async / no `IssueService`.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::fs::DirBuilder;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

pub(super) const SOCKET_PATH_LIMIT: usize = 100;

pub(super) fn socket_path_for(project_path: &Path) -> Result<PathBuf, String> {
    let file = format!(
        "{}.sock",
        short_hash(project_path.to_string_lossy().as_ref())
    );
    let primary = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("sworm/issues")
        .join(&file);
    if primary.as_os_str().len() <= SOCKET_PATH_LIMIT {
        return Ok(primary);
    }

    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    let fallback = std::env::temp_dir()
        .join(format!("sworm-{}", user))
        .join("issues")
        .join(file);
    if fallback.as_os_str().len() > SOCKET_PATH_LIMIT {
        return Err(format!(
            "Issue bridge socket path too long: {}",
            fallback.display()
        ));
    }
    Ok(fallback)
}

fn short_hash(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    hex_prefix(&digest, 12)
}

fn hex_prefix(bytes: &[u8], chars: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(chars);
    for byte in bytes {
        if out.len() >= chars {
            break;
        }
        out.push(HEX[(byte >> 4) as usize] as char);
        if out.len() >= chars {
            break;
        }
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Create `dir` (and any missing ancestors) with mode 0o700 set
/// atomically at creation, so the leaf directory is never world-
/// traversable even briefly. Falls back to a chmod after creation when
/// the dir already exists or on non-unix targets.
pub(super) fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        if dir.is_dir() {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
            return Ok(());
        }
        let mut builder = DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(dir)?;
        // Recursive creation only applies the mode to the leaf dir on
        // some platforms; tighten the leaf explicitly so callers always
        // see 0o700 regardless of which ancestor segments were new.
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}
