use super::{
    remote_runs::RemoteRunKind,
    remotes::merge_desktop_sections,
    target::{
        daemon_root_path, reject_remote, reject_remote_sources, remote_child_uri,
        remote_paste_source, remote_paste_sources, Target,
    },
    WorkspaceRouter,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use sworm_core::{
    errors::ApiError,
    events::{EventSink, HostEvent},
    services::pty::PtySubscriber,
};
use sworm_protocol::{
    pty::PtyEvent,
    rpc::{Reply, Request, RunStatus},
};

macro_rules! define_router_operation {
    (
        #[route(project_path)]
        FilesWatchDirs => $method:ident(
            $project_path:ident: $project_path_type:ty,
            $dirs:ident: $dirs_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            subscriber_id: String,
            $project_path: $project_path_type,
            $dirs: $dirs_type,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$project_path)? {
                self.inner.remember_claim(server, path, Some($dirs.clone()), false);
                return self
                    .call_reply(
                        server,
                        Request::FilesWatchDirs {
                            project_path: path.to_owned(),
                            dirs: $dirs,
                        },
                    )
                    .await?
                    .$method()
                    .map_err(ApiError::from);
            }
            self.local(move |host| host.$method(subscriber_id, $project_path, $dirs)).await
        }
    };
    (
        #[route(project_path)]
        GitWatch => $method:ident(
            $project_path:ident: $project_path_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $project_path: $project_path_type,
            owned: impl FnOnce(&Path) -> bool + Send + 'static,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$project_path)? {
                self.inner.remember_claim(server, path, None, true);
                return self
                    .call_reply(
                        server,
                        Request::GitWatch {
                            project_path: path.to_owned(),
                        },
                    )
                    .await?
                    .$method()
                    .map_err(ApiError::from);
            }
            self.local(move |host| host.$method($project_path, owned)).await
        }
    };
    (
        #[route(path)]
        FolderResolve => $method:ident($path:ident: $path_type:ty $(,)?) -> $return_type:ty;
    ) => {
        pub async fn $method(&self, $path: $path_type) -> Result<$return_type, ApiError> {
            if let Target::Remote {
                server,
                path: remote_path,
            } = Target::parse(&$path)?
            {
                self.inner.remember_claim(server, remote_path, None, false);
                let mut folder = self
                    .call_reply(
                        server,
                        Request::FolderResolve {
                            path: remote_path.to_owned(),
                        },
                    )
                    .await?
                    .$method()
                    .map_err(ApiError::from)?;
                folder.path = Target::remote_uri(server, &folder.path);
                return Ok(folder);
            }
            self.local(move |host| host.$method($path)).await
        }
    };
    (
        #[route(path)]
        FolderListEntries => $method:ident(
            $path:ident: $path_type:ty,
            $show_hidden:ident: $show_hidden_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $path: $path_type,
            $show_hidden: $show_hidden_type,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote {
                server,
                path: remote_path,
            } = Target::parse(&$path)?
            {
                // Browsing claims nothing; entries come back as remote URIs so
                // the switcher keeps navigating the same workspace.
                let mut entries = self
                    .call_reply(
                        server,
                        Request::FolderListEntries {
                            path: remote_path.to_owned(),
                            show_hidden: $show_hidden,
                        },
                    )
                    .await?
                    .$method()
                    .map_err(ApiError::from)?;
                for entry in &mut entries {
                    entry.path = Target::remote_uri(server, &entry.path);
                }
                return Ok(entries);
            }
            self.local(move |host| host.$method($path, $show_hidden)).await
        }
    };
    (
        #[route(folder_path)]
        SessionStart => $method:ident(
            $run_id:ident: $run_id_type:ty,
            $folder_path:ident: $folder_path_type:ty,
            $provider_id:ident: $provider_id_type:ty,
            $resume_token:ident: $resume_token_type:ty,
            $cols:ident: $cols_type:ty,
            $rows:ident: $rows_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        #[allow(clippy::too_many_arguments)]
        pub async fn $method(
            &self,
            $run_id: $run_id_type,
            $folder_path: $folder_path_type,
            $provider_id: $provider_id_type,
            $resume_token: $resume_token_type,
            $cols: $cols_type,
            $rows: $rows_type,
            output: EventSink<Vec<u8>>,
            events: EventSink<PtyEvent>,
            owner_id: Option<String>,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$folder_path)? {
                return self.start_remote_run(
                    RemoteRunKind::Session, $run_id.clone(), server, path, owner_id,
                        Request::SessionStart {
                            run_id: $run_id.clone(),
                            folder_path: path.to_owned(),
                            provider_id: $provider_id,
                            resume_token: $resume_token,
                            cols: $cols,
                            rows: $rows,
                        },
                    output, events, Reply::$method,
                ).await;
            }
            self.local(move |host| host.$method(
                    $run_id,
                    $folder_path,
                    $provider_id,
                    $resume_token,
                    $cols,
                    $rows,
                    Some(PtySubscriber { output, events }),
                    owner_id,
                ))
                .await
        }
    };
    (
        #[route(folder_path)]
        TasksStart => $method:ident(
            $run_id:ident: $run_id_type:ty,
            $folder_path:ident: $folder_path_type:ty,
            $task_id:ident: $task_id_type:ty,
            $active_file_path:ident: $active_file_path_type:ty,
            $cols:ident: $cols_type:ty,
            $rows:ident: $rows_type:ty,
            $attach_only:ident: $attach_only_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        #[allow(clippy::too_many_arguments)]
        pub async fn $method(
            &self,
            $run_id: $run_id_type,
            $folder_path: $folder_path_type,
            $task_id: $task_id_type,
            $active_file_path: $active_file_path_type,
            $cols: $cols_type,
            $rows: $rows_type,
            $attach_only: $attach_only_type,
            output: EventSink<Vec<u8>>,
            events: EventSink<PtyEvent>,
            owner_id: Option<String>,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$folder_path)? {
                return self.start_remote_run(
                    RemoteRunKind::Task, $run_id.clone(), server, path, owner_id,
                        Request::TasksStart {
                            run_id: $run_id.clone(),
                            folder_path: path.to_owned(),
                            task_id: $task_id,
                            active_file_path: $active_file_path,
                            cols: $cols,
                            rows: $rows,
                            attach_only: $attach_only,
                        },
                    output, events, Reply::$method,
                ).await;
            }
            self.local(move |host| host.$method(
                    $run_id,
                    $folder_path,
                    $task_id,
                    $active_file_path,
                    $cols,
                    $rows,
                    Some(PtySubscriber { output, events }),
                    owner_id,
                    $attach_only,
                ))
                .await
        }
    };
    (
        #[route(run)]
        SessionStop => $method:ident($run_id:ident: $run_id_type:ty $(,)?) -> $return_type:ty;
    ) => {
        pub async fn $method(&self, $run_id: $run_id_type) -> Result<$return_type, ApiError> {
            self.stop_registered($run_id, RemoteRunKind::Session).await
        }
    };
    (
        #[route(run)]
        TasksStop => $method:ident($run_id:ident: $run_id_type:ty $(,)?) -> $return_type:ty;
    ) => {
        pub async fn $method(&self, $run_id: $run_id_type) -> Result<$return_type, ApiError> {
            self.stop_registered($run_id, RemoteRunKind::Task).await
        }
    };
    (
        #[route(run)]
        RunStatus => $method:ident($run_id:ident: $run_id_type:ty $(,)?) -> $return_type:ty;
    ) => {
        pub async fn $method(&self, $run_id: $run_id_type) -> Result<$return_type, ApiError> {
            let Some(server) = self.inner.remote_runs.server_for(&$run_id) else {
                return self.local_run_status($run_id).await;
            };
            self.call_reply(&server, Request::RunStatus { run_id: $run_id })
                .await?
                .$method()
                .map_err(ApiError::from)
        }
    };
    (
        #[route(none)]
        AppRuntimeInfo => $method:ident() -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            name: String,
            version: String,
        ) -> Result<$return_type, ApiError> {
            self.local(move |host| host.$method(name, version)).await
        }
    };
    (
        #[route(folder_path)]
        FolderClaim => $method:ident($folder_path:ident: $folder_path_type:ty $(,)?) -> $return_type:ty;
    ) => {};
    (
        #[route(folder_path)]
        FolderRelease => $method:ident($folder_path:ident: $folder_path_type:ty $(,)?) -> $return_type:ty;
    ) => {};
    (#[route(connection)] $($rest:tt)*) => {};
    (
        #[route(none)]
        FolderPathRoot => $method:ident($path:ident: $path_type:ty $(,)?) -> $return_type:ty;
    ) => {
        pub async fn $method(&self, $path: $path_type) -> Result<$return_type, ApiError> {
            reject_remote("folder_path_root", &$path)?;
            self.local(move |host| host.$method($path)).await
        }
    };
    (
        #[route(none)]
        OmpResolveUri => $method:ident(
            $uri:ident: $uri_type:ty,
            $cwd:ident: $cwd_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $uri: $uri_type,
            $cwd: $cwd_type,
        ) -> Result<$return_type, ApiError> {
            if let Some(cwd) = &$cwd {
                reject_remote("omp_resolve_uri", cwd)?;
            }
            self.local(move |host| host.$method($uri, $cwd)).await
        }
    };
    (
        #[route(project_path)]
        FileRename => $method:ident(
            $project_path:ident: $project_path_type:ty,
            $old_path:ident: $old_path_type:ty,
            $new_path:ident: $new_path_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $project_path: $project_path_type,
            $old_path: $old_path_type,
            $new_path: $new_path_type,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$project_path)? {
                self.inner.remember_claim(server, path, None, false);
                self.call_reply(
                    server,
                    Request::FileRename {
                        project_path: path.to_owned(),
                        old_path: $old_path.clone(),
                        new_path: $new_path.clone(),
                    },
                )
                .await?
                .$method()
                .map_err(ApiError::from)?;
                return self.emit(HostEvent::FileMoved {
                    folder_path: PathBuf::from(Target::remote_uri(server, path)),
                    old_path: remote_child_uri(server, path, &$old_path),
                    new_path: remote_child_uri(server, path, &$new_path),
                    replace_destination: false,
                });
            }
            self.local(move |host| host.$method($project_path, $old_path, $new_path)).await
        }
    };
    (
        #[route(project_path)]
        FileDelete => $method:ident(
            $project_path:ident: $project_path_type:ty,
            $file_path:ident: $file_path_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $project_path: $project_path_type,
            $file_path: $file_path_type,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$project_path)? {
                self.inner.remember_claim(server, path, None, false);
                self.call_reply(
                    server,
                    Request::FileDelete {
                        project_path: path.to_owned(),
                        file_path: $file_path.clone(),
                    },
                )
                .await?
                .$method()
                .map_err(ApiError::from)?;
                return self.emit(HostEvent::FileDeleted(remote_child_uri(
                    server,
                    path,
                    &$file_path,
                )));
            }
            self.local(move |host| host.$method($project_path, $file_path)).await
        }
    };
    (
        #[route(project_path)]
        FilePaste => $method:ident(
            $project_path:ident: $project_path_type:ty,
            $target_dir:ident: $target_dir_type:ty,
            $op:ident: $op_type:ty,
            $sources:ident: $sources_type:ty,
            $collision_policy:ident: $collision_policy_type:ty,
            $rename_map:ident: $rename_map_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        #[allow(clippy::too_many_arguments)]
        pub async fn $method(
            &self,
            $project_path: $project_path_type,
            $target_dir: $target_dir_type,
            $op: $op_type,
            $sources: $sources_type,
            $collision_policy: $collision_policy_type,
            $rename_map: $rename_map_type,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$project_path)? {
                self.inner.remember_claim(server, path, None, false);
                let sources = remote_paste_sources(server, &$sources)?;
                let rename_map = match $rename_map {
                    Some(map) => Some(
                        map.into_iter()
                            .map(|(source, name)| {
                                remote_paste_source(server, &source).map(|source| (source, name))
                            })
                            .collect::<Result<HashMap<String, String>, ApiError>>()?,
                    ),
                    None => None,
                };
                let cut = $op == "cut";
                let mut mappings = self
                    .call_reply(
                        server,
                        Request::FilePaste {
                            project_path: path.to_owned(),
                            target_dir: $target_dir,
                            op: $op,
                            sources,
                            collision_policy: $collision_policy,
                            rename_map,
                        },
                    )
                    .await?
                    .$method()
                    .map_err(ApiError::from)?;
                for mapping in &mut mappings {
                    if cut {
                        self.emit(HostEvent::FileMoved {
                            folder_path: PathBuf::from(Target::remote_uri(server, path)),
                            old_path: PathBuf::from(Target::remote_uri(server, &mapping.source)),
                            new_path: remote_child_uri(server, path, &mapping.destination),
                            replace_destination: true,
                        })?;
                    }
                    // Hand back the URI the caller pasted, not the daemon path.
                    mapping.source = Target::remote_uri(server, &mapping.source);
                }
                return Ok(mappings);
            }
            reject_remote_sources(&$sources)?;
            self.local(move |host| host.$method(
                    $project_path,
                    $target_dir,
                    $op,
                    $sources,
                    $collision_policy,
                    $rename_map,
                ))
                .await
        }
    };
    (
        #[route(project_path)]
        FilePasteCollisions => $method:ident(
            $project_path:ident: $project_path_type:ty,
            $target_dir:ident: $target_dir_type:ty,
            $sources:ident: $sources_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $project_path: $project_path_type,
            $target_dir: $target_dir_type,
            $sources: $sources_type,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$project_path)? {
                self.inner.remember_claim(server, path, None, false);
                let sources = remote_paste_sources(server, &$sources)?;
                let mut collisions = self
                    .call_reply(
                        server,
                        Request::FilePasteCollisions {
                            project_path: path.to_owned(),
                            target_dir: $target_dir,
                            sources,
                        },
                    )
                    .await?
                    .$method()
                    .map_err(ApiError::from)?;
                for collision in &mut collisions {
                    collision.source = Target::remote_uri(server, &collision.source);
                }
                return Ok(collisions);
            }
            reject_remote_sources(&$sources)?;
            self.local(move |host| host.$method($project_path, $target_dir, $sources)).await
        }
    };
    (
        #[route(opt_folder_path)]
        SettingsGetEffective => $method:ident(
            $folder_path:ident: $folder_path_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        /// Host sections resolve where the folder lives; desktop sections
        /// describe this window, so the local layer wins for those.
        pub async fn $method(&self, $folder_path: $folder_path_type) -> Result<$return_type, ApiError> {
            if let Some(folder_path) = $folder_path.as_deref() {
                if let Target::Remote { server, path } = Target::parse(folder_path)? {
                    let mut remote = self
                        .call_reply(
                            server,
                            Request::SettingsGetEffective {
                                folder_path: Some(path.to_owned()),
                            },
                        )
                        .await?
                        .$method()
                        .map_err(ApiError::from)?;
                    let local = self.local(move |host| host.$method(None)).await?;
                    merge_desktop_sections(&mut remote, local, server);
                    return Ok(remote);
                }
            }
            self.local(move |host| host.$method($folder_path)).await
        }
    };
    (
        #[route(folder_path)]
        SettingsOpenFolderFile => $method:ident(
            $folder_path:ident: $folder_path_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(&self, $folder_path: $folder_path_type) -> Result<$return_type, ApiError> {
            let routed = match Target::parse(&$folder_path)? {
                Target::Local => None,
                Target::Remote { server, path } => Some((server.to_owned(), path.to_owned())),
            };
            if let Some((server, path)) = routed {
                self.inner.remember_claim(&server, &path, None, false);
                let mut result = self
                    .call_reply(&server, Request::SettingsOpenFolderFile { folder_path: path })
                    .await?
                    .$method()
                    .map_err(ApiError::from)?;
                // The file the caller opens next lives on the daemon.
                result.path = Target::remote_uri(&server, &result.path);
                return Ok(result);
            }
            self.local(move |host| host.$method($folder_path)).await
        }
    };
    (
        #[route(server)]
        $variant:ident => $method:ident(
            $($argument:ident: $argument_type:ty),* $(,)?
        ) -> $return_type:ty;
    ) => {
        /// Host-owned settings belong to whichever server runs the workspace
        /// being edited, so the caller names it instead of a path.
        pub async fn $method(
            &self,
            server: Option<String>,
            $($argument: $argument_type),*
        ) -> Result<$return_type, ApiError> {
            if let Some(server) = server.as_deref() {
                return self
                    .call_reply(server, Request::$variant { $($argument),* })
                    .await?
                    .$method()
                    .map_err(ApiError::from);
            }
            self.local(move |host| host.$method($($argument),*)).await
        }
    };
    (
        #[route(opt_folder_path)]
        $variant:ident => $method:ident(
            $folder_path:ident: $folder_path_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $folder_path: $folder_path_type,
        ) -> Result<$return_type, ApiError> {
            if let Some(folder_path) = $folder_path.as_deref() {
                if let Target::Remote { server, path } = Target::parse(folder_path)? {
                    self.inner.remember_claim(server, path, None, false);
                    return self
                        .call_reply(
                            server,
                            Request::$variant {
                                $folder_path: Some(path.to_owned()),
                            },
                        )
                        .await?
                        .$method()
                        .map_err(ApiError::from);
                }
            }
            self.local(move |host| host.$method($folder_path)).await
        }
    };
    (
        #[route(folder_path)]
        LspStart => $method:ident(
            $session_id:ident: $session_id_type:ty,
            $folder_path:ident: $folder_path_type:ty,
            $server_definition_id:ident: $server_definition_id_type:ty,
            $root_path:ident: $root_path_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            owner_id: Option<String>,
            $session_id: $session_id_type,
            $folder_path: $folder_path_type,
            $server_definition_id: $server_definition_id_type,
            $root_path: $root_path_type,
            events: EventSink<sworm_protocol::lsp::LspEvent>,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$folder_path)? {
                self.inner.remember_claim(server, path, None, false);
                // The language server only ever sees daemon-absolute paths.
                let root_path = daemon_root_path(server, &$root_path);
                // Register before starting: the daemon rejects a start whose
                // event stream is missing.
                self.inner
                    .remote_lsp
                    .attach(&self.inner, &$session_id, server, owner_id, events)
                    .await?;
                let result = self
                    .call_reply(
                        server,
                        Request::LspStart {
                            session_id: $session_id.clone(),
                            folder_path: path.to_owned(),
                            server_definition_id: $server_definition_id,
                            root_path,
                        },
                    )
                    .await
                    .and_then(|reply| reply.$method().map_err(ApiError::from));
                if result.is_err() {
                    self.inner.remote_lsp.cancel(&$session_id);
                }
                return result;
            }
            self.local(move |host| host.$method(
                    owner_id,
                    $session_id,
                    $folder_path,
                    $server_definition_id,
                    $root_path,
                    events,
                ))
                .await
        }
    };
    (
        #[route(lsp)]
        LspStop => $method:ident($session_id:ident: $session_id_type:ty $(,)?) -> $return_type:ty;
    ) => {
        pub async fn $method(&self, $session_id: $session_id_type) -> Result<$return_type, ApiError> {
            let Some(server) = self.inner.remote_lsp.server_for(&$session_id) else {
                return self.local(move |host| host.$method($session_id)).await;
            };
            // The stream is this session's lease: once it is gone the daemon
            // already killed the server, and a stop for the id could only reach
            // the session that replaced this one.
            if self.inner.remote_lsp.cancel_if_ended(&$session_id) {
                return Ok(Default::default());
            }
            let result = self
                .call_reply(
                    &server,
                    Request::LspStop {
                        session_id: $session_id.clone(),
                    },
                )
                .await
                    .and_then(|reply| reply.$method().map_err(ApiError::from));
            // Closing the stream is the backstop kill, so drop it either way.
            self.inner.remote_lsp.cancel(&$session_id);
            result
        }
    };
    (
        #[route(none)]
        $variant:ident => $method:ident(
            $($argument:ident: $argument_type:ty),* $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $($argument: $argument_type),*
        ) -> Result<$return_type, ApiError> {
            self.local(move |host| host.$method($($argument),*)).await
        }
    };
    (
        #[route($route:ident)]
        $variant:ident => $method:ident(
            $($argument:ident: $argument_type:ty),* $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $($argument: $argument_type),*
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$route)? {
                self.inner.remember_claim(server, path, None, false);
                let $route = path.to_owned();
                return self
                    .call_reply(server, Request::$variant { $($argument),* })
                    .await?
                    .$method()
                    .map_err(ApiError::from);
            }
            self.local(move |host| host.$method($($argument),*)).await
        }
    };
}

macro_rules! define_router_operations {
    (
        $(
            #[route($route:ident)]
            $variant:ident => $method:ident(
                $($argument:ident: $argument_type:ty),* $(,)?
            ) -> $return_type:ty;
        )*
    ) => {
        impl WorkspaceRouter {
            $(
                define_router_operation! {
                    #[route($route)]
                    $variant => $method(
                        $($argument: $argument_type),*
                    ) -> $return_type;
                }
            )*
        }
    };
}

sworm_protocol::sworm_rpc_ops!(define_router_operations);

impl WorkspaceRouter {
    async fn stop_registered(
        &self,
        run_id: String,
        local_kind: RemoteRunKind,
    ) -> Result<(), ApiError> {
        let Some(info) = self.inner.remote_runs.begin_stop(&run_id) else {
            return self.stop_local(local_kind, run_id).await;
        };
        let local_run_id = run_id.clone();
        let kind = info.kind;
        let (remote_result, local_result) = tokio::join!(
            self.inner
                .stop_backend_on(&info.server, &info.workbench, &run_id, info.kind),
            self.stop_local(kind, local_run_id)
        );
        self.inner.remote_runs.cancel(&run_id, info.generation);
        remote_result.and(local_result)
    }

    async fn local_run_status(&self, run_id: String) -> Result<RunStatus, ApiError> {
        self.local(move |host| host.run_status(&run_id)).await
    }
}

impl WorkspaceRouter {
    #[allow(clippy::too_many_arguments)]
    async fn start_remote_run<T>(
        &self,
        kind: RemoteRunKind,
        run_id: String,
        server: &str,
        path: &str,
        owner_id: Option<String>,
        request: Request,
        output: EventSink<Vec<u8>>,
        events: EventSink<PtyEvent>,
        decode: impl FnOnce(Reply) -> Result<T, sworm_protocol::rpc::WireError>,
    ) -> Result<T, ApiError> {
        self.inner.remote_runs.validate_start(
            &self.inner.host,
            &run_id,
            server,
            kind,
            owner_id.as_deref(),
        )?;
        self.inner.remember_claim(server, path, None, false);
        let _guard = self.inner.transition(server).lock_owned().await;
        let workbench = owner_id
            .as_deref()
            .and_then(|owner| {
                self.inner
                    .leases
                    .lock()
                    .0
                    .get(&(owner.to_owned(), server.to_owned()))
                    .map(|lease| lease.id.clone())
            })
            .ok_or_else(|| {
                ApiError::InvalidArgument(format!(
                    "Window has no attached workbench on remote `{server}`"
                ))
            })?;
        let reply = self
            .inner
            .call_reply_for(server, Some(&workbench), request)
            .await?;
        let result = decode(reply).map_err(ApiError::from)?;
        self.inner.remote_runs.adopt(
            Arc::downgrade(&self.inner),
            &self.inner.host,
            run_id,
            server.to_owned(),
            workbench,
            kind,
            output,
            events,
            owner_id,
        )?;
        Ok(result)
    }

    async fn stop_local(&self, kind: RemoteRunKind, run_id: String) -> Result<(), ApiError> {
        self.local(move |host| match kind {
            RemoteRunKind::Session => host.session_stop(run_id),
            RemoteRunKind::Task => host.tasks_stop(run_id),
        })
        .await
    }
}
