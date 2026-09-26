use anyhow::{bail, Context, Result};
use chrono::DateTime;
use std::{fs, os::unix::fs::PermissionsExt, path::Path, time::Duration};
use sworm_core::services::settings::SettingsService;
use sworm_protocol::{
    folder::PathRootKind,
    provider::{ProviderConnectionStatus, ProviderId},
    rpc::{HostEventFrame, HostEventWire, Open, RecentFolder, Request, WireError},
    settings::{PatchSettingsSectionInput, SettingsLayerKind},
};
use sworm_remote::{wire::read_frame, Identity, RemoteClient, RemoteError};
use sworm_server::{auth, serve, ServeOptions, ServerHandle};
use tempfile::TempDir;
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(5);
const KEY: &str = "workbench:phase1";

struct Daemon {
    config: TempDir,
    data: TempDir,
    endpoint: quinn::Endpoint,
    identity: Identity,
    handle: ServerHandle,
}

impl Daemon {
    async fn start() -> Result<Self> {
        let config = tempfile::tempdir()?;
        let data = tempfile::tempdir()?;
        let identity_dir = tempfile::tempdir()?;
        let identity = Identity::load_or_generate(identity_dir.path(), "client")?;
        let endpoint = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
        let handle = Self::serve(config.path(), data.path()).await?;
        Ok(Self {
            config,
            data,
            endpoint,
            identity,
            handle,
        })
    }

    async fn serve(config: &Path, data: &Path) -> Result<ServerHandle> {
        Ok(serve(ServeOptions {
            config_dir: config.to_path_buf(),
            data_dir: data.to_path_buf(),
            listen: Some("127.0.0.1:0".parse()?),
            config_file: None,
            web_assets_dir: None,
        })
        .await?)
    }

    async fn client(&self) -> Result<RemoteClient> {
        Ok(RemoteClient::connect(
            &self.endpoint,
            self.handle.local_addr,
            &self.identity,
            self.handle.fingerprint,
        )
        .await?)
    }

    async fn paired_client(&self) -> Result<RemoteClient> {
        let token = auth::write_pairing_token(self.config.path())?;
        let client = self.client().await?;
        client.pair(&token, "workbench test").await?;
        Ok(client)
    }

    async fn restart(&mut self) -> Result<()> {
        let old = std::mem::replace(
            &mut self.handle,
            Self::serve(self.config.path(), self.data.path()).await?,
        );
        old.shutdown().await;
        Ok(())
    }
}

fn path_string(path: &Path) -> String {
    path.to_str().expect("scratch path is UTF-8").to_owned()
}

fn assert_wire_error<T: std::fmt::Debug>(result: Result<T, RemoteError>, expected: &str) {
    let error = result.expect_err("request must fail");
    assert!(
        matches!(
            (&error, expected),
            (
                RemoteError::Wire(WireError::Unauthorized { .. }),
                "unauthorized"
            ) | (RemoteError::Wire(WireError::Database { .. }), "database")
                | (
                    RemoteError::Wire(WireError::InvalidArgument { .. }),
                    "invalid_argument"
                )
                | (RemoteError::Wire(WireError::NotFound { .. }), "not_found")
        ),
        "unexpected {expected} error: {error:?}"
    );
}

async fn kv_get(client: &RemoteClient, key: &str) -> Result<Option<String>, RemoteError> {
    client
        .call(&Request::AppStateGet {
            key: key.to_owned(),
        })
        .await?
        .app_state_get()
        .map_err(RemoteError::Wire)
}

async fn kv_put(client: &RemoteClient, key: &str, value_json: &str) -> Result<(), RemoteError> {
    client
        .call(&Request::AppStatePut {
            key: key.to_owned(),
            value_json: value_json.to_owned(),
        })
        .await?
        .app_state_put()
        .map_err(RemoteError::Wire)
}

async fn kv_delete(client: &RemoteClient, key: &str) -> Result<(), RemoteError> {
    client
        .call(&Request::AppStateDelete {
            key: key.to_owned(),
        })
        .await?
        .app_state_delete()
        .map_err(RemoteError::Wire)
}

async fn recent_list(client: &RemoteClient) -> Result<Vec<RecentFolder>, RemoteError> {
    client
        .call(&Request::RecentFoldersList {})
        .await?
        .recent_folders_list()
        .map_err(RemoteError::Wire)
}

async fn recent_touch(client: &RemoteClient, path: &str) -> Result<Vec<RecentFolder>, RemoteError> {
    client
        .call(&Request::RecentFoldersTouch {
            path: path.to_owned(),
        })
        .await?
        .recent_folders_touch()
        .map_err(RemoteError::Wire)
}

async fn recent_remove(
    client: &RemoteClient,
    paths: Vec<String>,
) -> Result<Vec<RecentFolder>, RemoteError> {
    client
        .call(&Request::RecentFoldersRemove { paths })
        .await?
        .recent_folders_remove()
        .map_err(RemoteError::Wire)
}

fn recent_paths(folders: &[RecentFolder]) -> Vec<String> {
    folders.iter().map(|folder| folder.path.clone()).collect()
}

async fn next_event(recv: &mut quinn::RecvStream) -> Result<HostEventWire> {
    Ok(read_frame::<HostEventFrame>(recv).await?.0)
}

async fn settings_barrier(
    client: &RemoteClient,
    first: &mut quinn::RecvStream,
    second: &mut quinn::RecvStream,
) -> Result<()> {
    // Keep each frame read alive across readiness retries: dropping a partial
    // read would lose the QUIC stream's frame boundary.
    timeout(WAIT, async {
        async fn ready(recv: &mut quinn::RecvStream) -> Result<()> {
            loop {
                if let HostEventWire::SettingsChanged(event) = next_event(recv).await? {
                    if event.layer == SettingsLayerKind::Global && event.folder_path.is_none() {
                        return Ok::<(), anyhow::Error>(());
                    }
                }
            }
        }
        let first = ready(first);
        let second = ready(second);
        tokio::pin!(first, second);
        let mut observed = [false, false];
        let mut attempts = tokio::time::interval(Duration::from_millis(200));
        let mut compact = false;
        while !observed.iter().all(|ready| *ready) {
            tokio::select! {
                result = &mut first, if !observed[0] => {
                    result?;
                    observed[0] = true;
                }
                result = &mut second, if !observed[1] => {
                    result?;
                    observed[1] = true;
                }
                _ = attempts.tick() => {
                    client.call(&Request::SettingsPatchGlobalSection {
                        input: PatchSettingsSectionInput {
                            section: "explorer".into(),
                            value: serde_json::json!({ "compact_folders": compact }),
                        },
                    }).await?.settings_patch_global_section().map_err(RemoteError::Wire)?;
                    compact = !compact;
                }
            }
        }
        Ok::<(), anyhow::Error>(())
    })
    .await
    .context("event streams did not become ready")??;
    Ok(())
}

async fn assert_recent_event(
    recv: &mut quinn::RecvStream,
    expected: &[RecentFolder],
) -> Result<()> {
    timeout(WAIT, async {
        loop {
            if let HostEventWire::RecentFoldersChanged(folders) = next_event(recv).await? {
                assert_eq!(folders, expected, "event must carry committed recent list");
                return Ok::<(), anyhow::Error>(());
            }
        }
    })
    .await
    .context("missing recent-folders event")??;
    Ok(())
}

async fn kv_and_recent(daemon: &mut Daemon) -> Result<()> {
    let unpaired = daemon.client().await?;
    assert_wire_error(kv_get(&unpaired, KEY).await, "unauthorized");
    unpaired.close();

    let client = daemon.paired_client().await?;
    assert_eq!(kv_get(&client, KEY).await?, None);
    kv_put(&client, KEY, "{  \"tabs\": [1]  }").await?;
    assert_eq!(
        kv_get(&client, KEY).await?.as_deref(),
        Some("{  \"tabs\": [1]  }")
    );
    let latest = "{\n  \"tabs\": [2],\n  \"mark\": \"latest\"\n}";
    kv_put(&client, KEY, latest).await?;
    client.close();
    let client = daemon.paired_client().await?;
    assert_eq!(kv_get(&client, KEY).await?.as_deref(), Some(latest));
    client.close();
    daemon.restart().await?;
    let client = daemon.paired_client().await?;
    assert_eq!(kv_get(&client, KEY).await?.as_deref(), Some(latest));
    let other = Daemon::start().await?;
    let other_client = other.paired_client().await?;
    assert_eq!(kv_get(&other_client, KEY).await?, None);
    other_client.close();
    other.handle.shutdown().await;
    kv_delete(&client, KEY).await?;
    kv_delete(&client, KEY).await?;
    assert_eq!(kv_get(&client, KEY).await?, None);

    assert_eq!(recent_list(&client).await?, Vec::<RecentFolder>::new());
    let touched = recent_touch(&client, "A").await?;
    assert_eq!(recent_paths(&touched), ["A"]);
    let first_opened = DateTime::parse_from_rfc3339(&touched[0].opened_at)?;
    assert_eq!(recent_paths(&recent_touch(&client, "B").await?), ["B", "A"]);
    let touched = recent_touch(&client, "A").await?;
    assert_eq!(recent_paths(&touched), ["A", "B"]);
    assert!(DateTime::parse_from_rfc3339(&touched[0].opened_at)? >= first_opened);
    let mut expected = vec!["A".to_owned(), "B".to_owned()];
    for n in 0..13 {
        let entry = format!("scratch-{n}");
        expected.retain(|path| path != &entry);
        expected.insert(0, entry.clone());
        expected.truncate(12);
        assert_eq!(
            recent_paths(&recent_touch(&client, &entry).await?),
            expected
        );
    }
    let mut listed = recent_list(&client).await?;
    assert_eq!(recent_paths(&listed), expected);
    let removed = vec!["scratch-12".into(), "scratch-5".into(), "unknown".into()];
    listed.retain(|folder| !removed.contains(&folder.path));
    assert_eq!(recent_remove(&client, removed).await?, listed);
    assert_eq!(recent_list(&client).await?, listed);

    let saved = serde_json::to_string(&listed)?;
    kv_put(&client, "recent_folders", "not json [").await?;
    assert_wire_error(recent_list(&client).await, "database");
    assert_wire_error(recent_touch(&client, "new").await, "database");
    assert_wire_error(
        recent_remove(&client, vec!["scratch-1".into()]).await,
        "database",
    );
    assert_eq!(
        kv_get(&client, "recent_folders").await?.as_deref(),
        Some("not json [")
    );
    kv_put(&client, "recent_folders", &saved).await?;
    assert_eq!(recent_list(&client).await?, listed);

    let second = daemon.paired_client().await?;
    let (_first_send, mut first_events) = client.open_stream(Open::Events).await?;
    let (_second_send, mut second_events) = second.open_stream(Open::Events).await?;
    settings_barrier(&client, &mut first_events, &mut second_events).await?;
    let entry = "sworm://remote/opaque";
    let touched = recent_touch(&client, entry).await?;
    assert_eq!(touched[0].path, entry);
    assert_eq!(touched[1..], listed[..]);
    assert_recent_event(&mut first_events, &touched).await?;
    assert_recent_event(&mut second_events, &touched).await?;
    let removed = recent_remove(&client, vec![entry.into()]).await?;
    assert_eq!(removed, touched[1..]);
    assert_recent_event(&mut first_events, &removed).await?;
    assert_recent_event(&mut second_events, &removed).await?;
    second.close();
    client.close();
    Ok(())
}

async fn shortcuts(client: &RemoteClient, home: &Path) -> Result<()> {
    let missing = client
        .call(&Request::ShortcutsGetGlobal {})
        .await?
        .shortcuts_get_global()
        .map_err(RemoteError::Wire)?;
    let path = SettingsService::global_shortcuts_path().map_err(anyhow::Error::msg)?;
    assert!(path.starts_with(home));
    assert_eq!(missing.path, path_string(&path));
    assert!(!missing.loaded);
    assert_eq!(missing.value, serde_json::json!({}));
    assert!(!path.exists());

    let created = client
        .call(&Request::ShortcutsCreateGlobalFile {})
        .await?
        .shortcuts_create_global_file()
        .map_err(RemoteError::Wire)?;
    assert_eq!(created.path, path_string(&path));
    let template = fs::read_to_string(&path)?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&template)?,
        serde_json::json!({"version":1,"bindings":[],"unboundCommands":[]})
    );
    let loaded = client
        .call(&Request::ShortcutsGetGlobal {})
        .await?
        .shortcuts_get_global()
        .map_err(RemoteError::Wire)?;
    assert!(loaded.loaded);
    assert_eq!(
        loaded.value,
        serde_json::from_str::<serde_json::Value>(&template)?
    );
    assert_eq!(
        client
            .call(&Request::ShortcutsCreateGlobalFile {})
            .await?
            .shortcuts_create_global_file()
            .map_err(RemoteError::Wire)?
            .path,
        path_string(&path)
    );
    assert_eq!(fs::read_to_string(&path)?, template);

    let value = serde_json::json!({"version":1,"bindings":[{"command":"toggle-command-palette","key":"Ctrl+P"}],"unboundCommands":[]});
    let set = client
        .call(&Request::ShortcutsSetGlobal {
            value: value.clone(),
        })
        .await?
        .shortcuts_set_global()
        .map_err(RemoteError::Wire)?;
    assert_eq!(set.path, path_string(&path));
    assert!(set.loaded);
    assert_eq!(set.value, value);
    let on_disk = fs::read_to_string(&path)?;
    assert_eq!(
        on_disk,
        format!("{}\n", serde_json::to_string_pretty(&value)?)
    );
    assert_eq!(
        client
            .call(&Request::ShortcutsCreateGlobalFile {})
            .await?
            .shortcuts_create_global_file()
            .map_err(RemoteError::Wire)?
            .path,
        path_string(&path)
    );
    assert_eq!(
        fs::read_to_string(&path)?,
        on_disk,
        "create must preserve custom bindings"
    );
    assert_eq!(
        client
            .call(&Request::ShortcutsGetGlobal {})
            .await?
            .shortcuts_get_global()
            .map_err(RemoteError::Wire)?
            .value,
        value
    );
    assert!(matches!(
        client.call(&Request::ShortcutsSetGlobal { value: serde_json::json!([]) })
            .await.and_then(|reply| reply.shortcuts_set_global().map_err(RemoteError::Wire)),
        Err(RemoteError::Wire(WireError::InvalidArgument { message }))
            if message == "Shortcuts file root must be an object"
    ));
    assert_eq!(fs::read_to_string(&path)?, on_disk);
    assert_eq!(
        client
            .call(&Request::ShortcutsGetGlobal {})
            .await?
            .shortcuts_get_global()
            .map_err(RemoteError::Wire)?
            .value,
        value
    );
    Ok(())
}

async fn catalogs_and_providers(client: &RemoteClient, home: &Path) -> Result<()> {
    let catalog = client
        .call(&Request::BuiltinsGetCatalog {})
        .await?
        .builtins_get_catalog()
        .map_err(RemoteError::Wire)?;
    assert!(catalog
        .runtime
        .languages
        .iter()
        .any(|language| language.id == "nix"));
    assert!(catalog.settings.pages.iter().any(|page| page
        .server_definition_ids
        .iter()
        .any(|id| id == "dev.sworm.nix::nil")));

    let schemas = client
        .call(&Request::ConfigSchemasList {})
        .await?
        .config_schemas_list()
        .map_err(RemoteError::Wire)?;
    for id in [
        "sworm.settings.global",
        "sworm.settings.folder",
        "sworm.tasks",
        "sworm.shortcuts",
    ] {
        let schema = schemas
            .iter()
            .find(|entry| entry.id == id)
            .with_context(|| format!("missing schema {id}"))?;
        let wire = serde_json::to_value(schema)?;
        assert_eq!(wire["fileMatch"], serde_json::json!(schema.file_match));
        assert!(wire.get("file_match").is_none());
    }
    let shortcuts_schema = schemas
        .iter()
        .find(|entry| entry.id == "sworm.shortcuts")
        .unwrap();
    assert_eq!(
        shortcuts_schema
            .schema
            .pointer("/properties/bindings/items/required"),
        Some(&serde_json::json!(["command", "key"]))
    );

    let binary = home.join("phase1-omp");
    fs::write(&binary, "#!/bin/sh\nprintf 'phase1-omp 1.0\\n'\n")?;
    let mut permissions = fs::metadata(&binary)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&binary, permissions)?;
    let absent = path_string(&home.join("no-such-cli"));
    let config = serde_json::json!({
        "omp": {"binary_path_override": path_string(&binary)},
        "claude_code": {"binary_path_override": absent},
        "codex": {"binary_path_override": absent},
        "antigravity": {"binary_path_override": absent}
    });
    client
        .call(&Request::SettingsPatchGlobalSection {
            input: PatchSettingsSectionInput {
                section: "providers".into(),
                value: config,
            },
        })
        .await?
        .settings_patch_global_section()
        .map_err(RemoteError::Wire)?;
    let statuses = client
        .call(&Request::ProviderList {})
        .await?
        .provider_list()
        .map_err(RemoteError::Wire)?;
    let omp = statuses
        .iter()
        .find(|status| status.id == ProviderId::Omp)
        .context("missing OMP provider")?;
    assert!(matches!(omp.status, ProviderConnectionStatus::Connected));
    assert_eq!(omp.version.as_deref(), Some("phase1-omp 1.0"));
    assert_eq!(omp.resolved_path.as_deref(), Some(binary.to_str().unwrap()));
    for provider_id in [
        ProviderId::ClaudeCode,
        ProviderId::Codex,
        ProviderId::Antigravity,
    ] {
        let provider = statuses
            .iter()
            .find(|status| status.id == provider_id)
            .context("missing provider")?;
        assert!(matches!(provider.status, ProviderConnectionStatus::Error));
        assert_eq!(provider.resolved_path.as_deref(), Some(absent.as_str()));
    }
    Ok(())
}

async fn activity_and_root(client: &RemoteClient, home: &Path) -> Result<()> {
    let project = home.join("project");
    fs::create_dir(&project)?;
    assert!(client
        .call(&Request::ActivityMapGet {})
        .await?
        .activity_map_get()
        .map_err(RemoteError::Wire)?
        .is_empty());
    let session_dir = home.join(".omp/agent/sessions/-project");
    fs::create_dir_all(&session_dir)?;
    let stem = "2026-01-01T00-00-00-000Z_phase1";
    let jsonl = session_dir.join(format!("{stem}.jsonl"));
    fs::write(
        &jsonl,
        format!(
            "{}\n",
            serde_json::json!({"type":"session","cwd":path_string(&project)})
        ),
    )?;
    let artifact = session_dir.join(stem).join("local/probe.md");
    fs::create_dir_all(artifact.parent().unwrap())?;
    fs::write(&artifact, "probe\n")?;
    let projects = client
        .call(&Request::ActivityMapRefresh {})
        .await?
        .activity_map_refresh()
        .map_err(RemoteError::Wire)?;
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].path, path_string(&project));
    assert_eq!(projects[0].name, "project");
    assert!(projects[0].path_exists);
    assert_eq!(projects[0].providers.len(), 1);
    assert_eq!(projects[0].providers[0].provider_id, "omp");

    let target = client
        .call(&Request::OmpResolveUri {
            uri: "local://probe.md".into(),
            cwd: Some(path_string(&project)),
        })
        .await?
        .omp_resolve_uri()
        .map_err(RemoteError::Wire)?;
    assert_eq!(target.path, path_string(&artifact));
    assert!(!target.is_dir);
    assert!(matches!(
        client.call(&Request::OmpResolveUri {
            uri: "unsupported://probe".into(), cwd: Some(path_string(&project)),
        }).await.and_then(|reply| reply.omp_resolve_uri().map_err(RemoteError::Wire)),
        Err(RemoteError::Wire(WireError::InvalidArgument { message }))
            if message == "Unsupported scheme: unsupported"
    ));
    fs::remove_file(jsonl)?;
    assert_eq!(
        client
            .call(&Request::ActivityMapGet {})
            .await?
            .activity_map_get()
            .map_err(RemoteError::Wire)?[0]
            .path,
        path_string(&project)
    );
    assert!(client
        .call(&Request::ActivityMapRefresh {})
        .await?
        .activity_map_refresh()
        .map_err(RemoteError::Wire)?
        .is_empty());

    let root = client
        .call(&Request::FolderPathRoot {
            path: path_string(home),
        })
        .await?
        .folder_path_root()
        .map_err(RemoteError::Wire)?;
    assert!(matches!(root.kind, PathRootKind::Home));
    assert_eq!(root.path, path_string(&fs::canonicalize(home)?));
    assert_wire_error(
        client
            .call(&Request::FolderPathRoot {
                path: path_string(&home.join("missing-folder")),
            })
            .await
            .and_then(|reply| reply.folder_path_root().map_err(RemoteError::Wire)),
        "not_found",
    );
    Ok(())
}

async fn runtime_info(client: &RemoteClient) -> Result<()> {
    let runtime = client
        .call(&Request::AppRuntimeInfo {})
        .await?
        .app_runtime_info()
        .map_err(RemoteError::Wire)?;
    assert_eq!(runtime.name, "sworm-server");
    assert_eq!(runtime.version, env!("CARGO_PKG_VERSION"));
    Ok(())
}

#[test]
fn workbench_operations_over_paired_quic() -> Result<()> {
    // Global path resolution and process environment must be isolated before
    // the Tokio runtime, Host, server, or provider detector is constructed.
    let home = tempfile::tempdir()?;
    let config = home.path().join("config");
    let data = home.path().join("data");
    fs::create_dir_all(&config)?;
    fs::create_dir_all(&data)?;
    let git_config = home.path().join("gitconfig");
    fs::write(&git_config, "")?;
    std::env::set_var("HOME", home.path());
    std::env::set_var("XDG_CONFIG_HOME", &config);
    std::env::set_var("XDG_DATA_HOME", &data);
    std::env::set_var("GIT_CONFIG_GLOBAL", &git_config);
    std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
    for path in [
        SettingsService::global_settings_path(),
        SettingsService::global_shortcuts_path(),
    ] {
        let path = path.map_err(anyhow::Error::msg)?;
        if !path.starts_with(home.path()) {
            bail!("global file escaped scratch HOME: {}", path.display());
        }
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let mut daemon = Daemon::start().await?;
        kv_and_recent(&mut daemon).await?;
        let client = daemon.paired_client().await?;
        shortcuts(&client, home.path()).await?;
        catalogs_and_providers(&client, home.path()).await?;
        activity_and_root(&client, home.path()).await?;
        runtime_info(&client).await?;
        client.close();
        daemon.handle.shutdown().await;
        Ok(())
    })
}
