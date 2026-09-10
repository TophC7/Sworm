use crate::auth;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
use sworm_core::Host;
use sworm_protocol::rpc::{Request, Response, WireError, MAX_REMOTE_FILE_BYTES};
use sworm_remote::Fingerprint;
use tokio::sync::Mutex;

pub(crate) struct ServerContext {
    pub config_dir: PathBuf,
    pub auth_token: Option<String>,
    pub pairing: Mutex<()>,
    pub folders: parking_lot::Mutex<HashMap<PathBuf, usize>>,
}

impl ServerContext {
    pub fn claim_folder(&self, folder: &Path) {
        *self.folders.lock().entry(folder.to_path_buf()).or_insert(0) += 1;
    }

    /// Returns true when this was the last claim, so the caller releases Host resources.
    pub fn release_folder(&self, folder: &Path) -> bool {
        let mut folders = self.folders.lock();
        let Some(count) = folders.get_mut(folder) else {
            return false;
        };
        *count -= 1;
        if *count == 0 {
            folders.remove(folder);
            true
        } else {
            false
        }
    }
}

pub(crate) struct Session {
    pub fingerprint: Fingerprint,
    pub authorized: bool,
    pub folders: HashSet<PathBuf>,
}

pub(crate) async fn handle(
    host: &Host,
    ctx: &ServerContext,
    session: &Mutex<Session>,
    request: Request,
) -> Response {
    let request = match request {
        Request::Pair { token, name } => return pair(ctx, session, &token, &name).await,
        request => request,
    };

    if !session.lock().await.authorized {
        return Err(unauthorized("client is not paired with this server"));
    }

    match request {
        Request::FilesReadDir {
            project_path,
            dir_path,
            show_hidden,
        } => {
            require_absolute(&project_path, "project_path")?;
            let folder = PathBuf::from(&project_path);
            // Re-called per request: the folder watcher only attaches once `.sworm/` exists.
            host.watch_settings_paths(Some(&folder));
            if session.lock().await.folders.insert(folder.clone()) {
                ctx.claim_folder(&folder);
            }
            host.files_read_dir(project_path, dir_path, show_hidden)
                .await
                .map_err(WireError::from)
                .and_then(to_value)
        }
        Request::FileRead {
            project_path,
            file_path,
        } => {
            require_absolute(&project_path, "project_path")?;
            host.file_read_limited(project_path, file_path, MAX_REMOTE_FILE_BYTES)
                .await
                .map_err(WireError::from)
                .and_then(to_value)
        }
        Request::GitGetSummary { path } => {
            require_absolute(&path, "path")?;
            host.git_get_summary(path)
                .await
                .map_err(WireError::from)
                .and_then(to_value)
        }
        Request::Pair { .. } => unreachable!(),
    }
}

async fn pair(ctx: &ServerContext, session: &Mutex<Session>, token: &str, name: &str) -> Response {
    if session.lock().await.authorized {
        return Ok(serde_json::Value::Null);
    }

    let _pairing = ctx.pairing.lock().await;
    if session.lock().await.authorized {
        return Ok(serde_json::Value::Null);
    }
    let fingerprint = session.lock().await.fingerprint;
    let config_dir = ctx.config_dir.clone();
    let static_token = ctx.auth_token.clone();
    let (token, name) = (token.to_owned(), name.to_owned());
    let persisted = tokio::task::spawn_blocking(move || -> Result<bool, WireError> {
        let valid = auth::consume_pairing_token(&config_dir, &token, static_token.as_deref())
            .map_err(|error| WireError::Io {
                message: format!("consume pairing token: {error}"),
            })?;
        if !valid {
            return Ok(false);
        }
        auth::append_authorized(&config_dir, fingerprint, &name).map_err(|error| {
            WireError::Io {
                message: format!("persist authorized client: {error}"),
            }
        })?;
        Ok(true)
    })
    .await
    .map_err(|error| WireError::Internal {
        message: format!("pairing task: {error}"),
    })??;
    if !persisted {
        return Err(unauthorized("invalid or expired pairing token"));
    }
    session.lock().await.authorized = true;
    Ok(serde_json::Value::Null)
}

fn require_absolute(path: &str, field: &str) -> Result<(), WireError> {
    if Path::new(path).is_absolute() {
        Ok(())
    } else {
        Err(WireError::InvalidArgument {
            message: format!("remote {field} must be absolute: {path}"),
        })
    }
}

fn unauthorized(message: &str) -> WireError {
    WireError::Unauthorized {
        message: message.to_string(),
    }
}

fn to_value(value: impl serde::Serialize) -> Response {
    serde_json::to_value(value).map_err(|error| WireError::Internal {
        message: format!("encode response: {error}"),
    })
}
