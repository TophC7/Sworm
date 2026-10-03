use std::path::PathBuf;
use sworm_core::errors::ApiError;
use sworm_remote::RemoteError;

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

    pub fn remote_uri(server: &str, path: &str) -> String {
        format!("sworm://{server}/{}", path.trim_start_matches('/'))
    }
}

/// Absolute URI of a workspace-relative path inside a remote folder.
pub(super) fn remote_child_uri(server: &str, folder: &str, relative: &str) -> PathBuf {
    let folder = folder.trim_end_matches('/');
    let relative = relative.trim_start_matches('/');
    PathBuf::from(Target::remote_uri(server, &format!("{folder}/{relative}")))
}

/// Clipboard sources name files on the host that owns them. A local path here
/// would be a file the daemon cannot see, and another server's URI a file
/// neither host can reach, so both are refused instead of silently pasting
/// whatever happens to exist at that path on the daemon.
pub(super) fn remote_paste_source(server: &str, source: &str) -> Result<String, ApiError> {
    match Target::parse(source)? {
        Target::Remote {
            server: origin,
            path,
        } if origin == server => Ok(path.to_owned()),
        Target::Remote { server: origin, .. } => Err(ApiError::Remote(format!(
            "cannot paste files from `{origin}` into a workspace on `{server}`"
        ))),
        Target::Local => Err(ApiError::Remote(format!(
            "pasting local files into a remote workspace is not supported: {source}"
        ))),
    }
}

pub(super) fn remote_paste_sources(
    server: &str,
    sources: &[String],
) -> Result<Vec<String>, ApiError> {
    sources
        .iter()
        .map(|source| remote_paste_source(server, source))
        .collect()
}

/// Refuse a remote target for a command that can only act on this machine.
pub fn reject_remote(command: &str, path: &str) -> Result<(), ApiError> {
    match Target::parse(path)? {
        Target::Local => Ok(()),
        Target::Remote { .. } => Err(ApiError::Remote(format!(
            "{command} is not supported on remote workspaces"
        ))),
    }
}

pub(super) fn not_controller(workbench: &str) -> ApiError {
    ApiError::from(sworm_protocol::rpc::WireError::NotController {
        workbench: workbench.to_owned(),
    })
}

pub(crate) fn remote_error(server: &str, error: RemoteError) -> ApiError {
    match error {
        RemoteError::Wire(error) => ApiError::from(error),
        RemoteError::Connection(message)
        | RemoteError::Timeout(message)
        | RemoteError::Transport(message)
        | RemoteError::Identity(message) => ApiError::Remote(format!("{server}: {message}")),
    }
}

/// Strip the workspace URI from a root path: LSP servers only ever see
/// daemon-absolute paths.
pub(crate) fn daemon_root_path(server: &str, root_path: &str) -> String {
    match Target::parse(root_path) {
        Ok(Target::Remote {
            server: owner,
            path,
        }) if owner == server => path.to_owned(),
        _ => root_path.to_owned(),
    }
}

pub(super) fn reject_remote_sources(sources: &[String]) -> Result<(), ApiError> {
    for source in sources {
        if let Target::Remote { server, .. } = Target::parse(source)? {
            return Err(ApiError::Remote(format!(
                "pasting remote files from `{server}` into a local workspace is not supported"
            )));
        }
    }
    Ok(())
}
