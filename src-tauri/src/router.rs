use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use std::{collections::HashMap, str::FromStr, sync::Arc};
use sworm_core::{
    errors::ApiError,
    services::{
        settings::SettingsService, settings_resolution::resolve_effective_settings_for_folder_path,
    },
    Host,
};
use sworm_protocol::{files::DirEntry, git::GitSummary, rpc::Request};
use sworm_remote::{Fingerprint, Identity, RemoteClient, RemoteError};
use tokio::sync::{Mutex as AsyncMutex, OnceCell};

pub enum Target<'a> {
    Local,
    Remote { server: &'a str, path: &'a str },
}

impl<'a> Target<'a> {
    /// Parse a remote workspace URI; ordinary filesystem paths stay local.
    pub fn parse(project_path: &'a str) -> Result<Self, ApiError> {
        let Some(remote_path) = project_path.strip_prefix("sworm://") else {
            return Ok(Self::Local);
        };
        let Some(separator) = remote_path.find('/') else {
            return Err(ApiError::InvalidArgument(format!(
                "Invalid remote path: {project_path}"
            )));
        };
        let server = &remote_path[..separator];
        if server.is_empty() {
            return Err(ApiError::InvalidArgument(format!(
                "Invalid remote path: {project_path}"
            )));
        }
        Ok(Self::Remote {
            server,
            path: &remote_path[separator..],
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
struct RemoteConfig {
    address: String,
    fingerprint: Fingerprint,
}

struct CachedRemote {
    config: RemoteConfig,
    client: Arc<RemoteClient>,
}

type RemoteSlot = Arc<AsyncMutex<Option<CachedRemote>>>;

pub struct WorkspaceRouter {
    host: Arc<Host>,
    remotes: Mutex<HashMap<String, RemoteSlot>>,
    identity: OnceCell<Arc<Identity>>,
}

impl WorkspaceRouter {
    pub fn new(host: Arc<Host>) -> Self {
        Self {
            host,
            remotes: Mutex::new(HashMap::new()),
            identity: OnceCell::new(),
        }
    }

    pub async fn files_read_dir(
        &self,
        project_path: String,
        dir_path: String,
        show_hidden: bool,
    ) -> Result<Vec<DirEntry>, ApiError> {
        if let Target::Remote { server, path } = Target::parse(&project_path)? {
            return self
                .call(
                    server,
                    Request::FilesReadDir {
                        project_path: path.to_owned(),
                        dir_path,
                        show_hidden,
                    },
                )
                .await;
        }
        self.host
            .files_read_dir(project_path, dir_path, show_hidden)
            .await
    }

    pub async fn file_read(
        &self,
        project_path: String,
        file_path: String,
    ) -> Result<String, ApiError> {
        if let Target::Remote { server, path } = Target::parse(&project_path)? {
            return self
                .call(
                    server,
                    Request::FileRead {
                        project_path: path.to_owned(),
                        file_path,
                    },
                )
                .await;
        }
        self.host.file_read(project_path, file_path).await
    }

    pub async fn git_get_summary(&self, path: String) -> Result<GitSummary, ApiError> {
        if let Target::Remote {
            server,
            path: remote_path,
        } = Target::parse(&path)?
        {
            return self
                .call(
                    server,
                    Request::GitGetSummary {
                        path: remote_path.to_owned(),
                    },
                )
                .await;
        }
        self.host.git_get_summary(path).await
    }

    /// Retry once only when opening the request stream proves no request was sent.
    async fn call<T: DeserializeOwned>(
        &self,
        server: &str,
        request: Request,
    ) -> Result<T, ApiError> {
        let client = self.client(server).await?;
        match client.call(&request).await {
            Ok(value) => return Ok(value),
            Err(RemoteError::Connection(_)) => self.evict(server, &client).await,
            Err(error) => return Err(remote_error(server, error)),
        }

        let client = self.client(server).await?;
        let result = client.call(&request).await;
        if matches!(result, Err(RemoteError::Connection(_))) {
            self.evict(server, &client).await;
        }
        result.map_err(|error| remote_error(server, error))
    }

    async fn client(&self, server: &str) -> Result<Arc<RemoteClient>, ApiError> {
        let slot = {
            let mut remotes = self.remotes.lock();
            Arc::clone(
                remotes
                    .entry(server.to_owned())
                    .or_insert_with(|| Arc::new(AsyncMutex::new(None))),
            )
        };
        let mut cached = slot.lock().await;
        let server_owned = server.to_owned();
        let resolved = tokio::task::spawn_blocking(move || resolve_remote_config(&server_owned))
            .await
            .map_err(|error| ApiError::Internal(error.to_string()))?;
        let config = match resolved {
            Ok(config) => config,
            Err(error) => {
                if let Some(stale) = cached.take() {
                    stale.client.close();
                }
                drop(cached);
                let mut remotes = self.remotes.lock();
                if Arc::strong_count(&slot) == 2
                    && remotes
                        .get(server)
                        .is_some_and(|current| Arc::ptr_eq(current, &slot))
                {
                    remotes.remove(server);
                }
                return Err(error);
            }
        };
        if let Some(existing) = cached.as_ref() {
            if existing.config == config && !existing.client.is_closed() {
                return Ok(Arc::clone(&existing.client));
            }
        }
        if let Some(stale) = cached.take() {
            stale.client.close();
        }

        let mut addresses = tokio::net::lookup_host(config.address.as_str())
            .await
            .map_err(|error| {
                ApiError::Remote(format!(
                    "{server}: cannot resolve {}: {error}",
                    config.address
                ))
            })?
            .peekable();
        if addresses.peek().is_none() {
            return Err(ApiError::Remote(format!(
                "{server}: cannot resolve {}",
                config.address
            )));
        }
        let identity = self
            .identity
            .get_or_try_init(|| async {
                tokio::task::spawn_blocking(|| {
                    let dir = SettingsService::global_config_dir().map_err(ApiError::Internal)?;
                    Identity::load_or_generate(&dir, "client")
                        .map(Arc::new)
                        .map_err(|error| ApiError::Remote(format!("client identity: {error}")))
                })
                .await
                .map_err(|error| ApiError::Internal(error.to_string()))?
            })
            .await?;
        let identity = Arc::clone(identity);
        let mut last_error = None;
        let mut connected = None;
        for address in addresses {
            match RemoteClient::connect(address, &identity, config.fingerprint).await {
                Ok(client) => {
                    connected = Some(Arc::new(client));
                    break;
                }
                Err(error) => last_error = Some(error),
            }
        }
        let client = connected.ok_or_else(|| {
            remote_error(
                server,
                last_error.expect("resolved address list was non-empty"),
            )
        })?;
        *cached = Some(CachedRemote {
            config,
            client: Arc::clone(&client),
        });
        Ok(client)
    }

    async fn evict(&self, server: &str, failed: &Arc<RemoteClient>) {
        let slot = self.remotes.lock().get(server).cloned();
        let Some(slot) = slot else {
            return;
        };
        let mut cached = slot.lock().await;
        if cached
            .as_ref()
            .is_some_and(|cached| Arc::ptr_eq(&cached.client, failed))
        {
            if let Some(stale) = cached.take() {
                stale.client.close();
            }
        }
    }
}

fn resolve_remote_config(server: &str) -> Result<RemoteConfig, ApiError> {
    let resolved = resolve_effective_settings_for_folder_path(None).map_err(ApiError::Internal)?;
    let remote = resolved
        .settings
        .remotes
        .get(server)
        .ok_or_else(|| ApiError::NotFound(format!("Unknown remote server `{server}`")))?;
    let fingerprint = Fingerprint::from_str(&remote.fingerprint).map_err(|_| {
        ApiError::InvalidArgument(format!("Invalid fingerprint for remote `{server}`"))
    })?;
    Ok(RemoteConfig {
        address: remote.address.clone(),
        fingerprint,
    })
}

fn remote_error(server: &str, error: RemoteError) -> ApiError {
    match error {
        RemoteError::Wire(error) => ApiError::from(error),
        RemoteError::Connection(message)
        | RemoteError::Transport(message)
        | RemoteError::Identity(message) => ApiError::Remote(format!("{server}: {message}")),
    }
}
