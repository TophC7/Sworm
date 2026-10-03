//! Host operations run in scratch HOME/XDG child processes before constructing a runtime.
mod support;

use anyhow::{bail, Context, Result};
use chrono::DateTime;
use std::{fs, os::unix::fs::PermissionsExt, path::Path, time::Duration};
use support::Daemon;
use sworm_core::services::settings::SettingsService;
use sworm_protocol::{
    folder::PathRootKind,
    provider::{ProviderConnectionStatus, ProviderId},
    rpc::{HostEventWire, Open, RecentFolder, Reply, Request, WireError},
    settings::{
        EffectiveSettings, ExternalFileOpenMode, ExternalFolderOpenMode, PatchSettingsSectionInput,
        SettingsLayerKind, TabBeamPosition, TerminalSettings, WindowSettings,
    },
};
use sworm_remote::{wire::read_frame, RemoteClient, RemoteError};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(5);
const KEY: &str = "workbench:state";

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

fn recent_paths(folders: &[RecentFolder]) -> Vec<String> {
    folders.iter().map(|folder| folder.path.clone()).collect()
}

async fn next_event(recv: &mut quinn::RecvStream) -> Result<HostEventWire> {
    Ok(read_frame::<HostEventWire>(recv).await?)
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
    assert_wire_error(
        unpaired
            .call_as(
                &Request::AppStateGet {
                    key: (KEY).to_owned(),
                },
                Reply::app_state_get,
            )
            .await,
        "unauthorized",
    );
    unpaired.close();

    let client = daemon.paired_client().await?;
    assert_eq!(
        client
            .call_as(
                &Request::AppStateGet {
                    key: KEY.to_owned()
                },
                Reply::app_state_get
            )
            .await?,
        None
    );
    client
        .call_as(
            &Request::AppStatePut {
                key: (KEY).to_owned(),
                value_json: ("{  \"tabs\": [1]  }").to_owned(),
            },
            Reply::app_state_put,
        )
        .await?;
    assert_eq!(
        client
            .call_as(
                &Request::AppStateGet {
                    key: KEY.to_owned()
                },
                Reply::app_state_get
            )
            .await?
            .as_deref(),
        Some("{  \"tabs\": [1]  }")
    );
    let latest = "{\n  \"tabs\": [2],\n  \"mark\": \"latest\"\n}";
    client
        .call_as(
            &Request::AppStatePut {
                key: (KEY).to_owned(),
                value_json: (latest).to_owned(),
            },
            Reply::app_state_put,
        )
        .await?;
    client.close();
    let client = daemon.paired_client().await?;
    assert_eq!(
        client
            .call_as(
                &Request::AppStateGet {
                    key: KEY.to_owned()
                },
                Reply::app_state_get
            )
            .await?
            .as_deref(),
        Some(latest)
    );
    client.close();
    daemon.restart().await?;
    let client = daemon.paired_client().await?;
    assert_eq!(
        client
            .call_as(
                &Request::AppStateGet {
                    key: KEY.to_owned()
                },
                Reply::app_state_get
            )
            .await?
            .as_deref(),
        Some(latest)
    );
    let other = Daemon::start(None).await?;
    let other_client = other.paired_client().await?;
    assert_eq!(
        other_client
            .call_as(
                &Request::AppStateGet {
                    key: KEY.to_owned()
                },
                Reply::app_state_get
            )
            .await?,
        None
    );
    other_client.close();
    other.handle.shutdown().await;
    client
        .call_as(
            &Request::AppStateDelete {
                key: (KEY).to_owned(),
            },
            Reply::app_state_delete,
        )
        .await?;
    client
        .call_as(
            &Request::AppStateDelete {
                key: (KEY).to_owned(),
            },
            Reply::app_state_delete,
        )
        .await?;
    assert_eq!(
        client
            .call_as(
                &Request::AppStateGet {
                    key: KEY.to_owned()
                },
                Reply::app_state_get
            )
            .await?,
        None
    );

    assert_eq!(
        client
            .call_as(&Request::RecentFoldersList {}, Reply::recent_folders_list)
            .await?,
        Vec::<RecentFolder>::new()
    );
    let touched = client
        .call_as(
            &Request::RecentFoldersTouch {
                path: ("A").to_owned(),
            },
            Reply::recent_folders_touch,
        )
        .await?;
    assert_eq!(recent_paths(&touched), ["A"]);
    let first_opened = DateTime::parse_from_rfc3339(&touched[0].opened_at)?;
    assert_eq!(
        recent_paths(
            &client
                .call_as(
                    &Request::RecentFoldersTouch {
                        path: "B".to_owned()
                    },
                    Reply::recent_folders_touch
                )
                .await?
        ),
        ["B", "A"]
    );
    let touched = client
        .call_as(
            &Request::RecentFoldersTouch {
                path: ("A").to_owned(),
            },
            Reply::recent_folders_touch,
        )
        .await?;
    assert_eq!(recent_paths(&touched), ["A", "B"]);
    assert!(DateTime::parse_from_rfc3339(&touched[0].opened_at)? >= first_opened);
    let mut expected = vec!["A".to_owned(), "B".to_owned()];
    for n in 0..13 {
        let entry = format!("scratch-{n}");
        expected.retain(|path| path != &entry);
        expected.insert(0, entry.clone());
        expected.truncate(12);
        assert_eq!(
            recent_paths(
                &client
                    .call_as(
                        &Request::RecentFoldersTouch {
                            path: entry.clone()
                        },
                        Reply::recent_folders_touch
                    )
                    .await?
            ),
            expected
        );
    }
    let mut listed = client
        .call_as(&Request::RecentFoldersList {}, Reply::recent_folders_list)
        .await?;
    assert_eq!(recent_paths(&listed), expected);
    let removed = vec!["scratch-12".into(), "scratch-5".into(), "unknown".into()];
    listed.retain(|folder| !removed.contains(&folder.path));
    assert_eq!(
        client
            .call_as(
                &Request::RecentFoldersRemove { paths: removed },
                Reply::recent_folders_remove
            )
            .await?,
        listed
    );
    assert_eq!(
        client
            .call_as(&Request::RecentFoldersList {}, Reply::recent_folders_list)
            .await?,
        listed
    );

    let saved = serde_json::to_string(&listed)?;
    client
        .call_as(
            &Request::AppStatePut {
                key: ("recent_folders").to_owned(),
                value_json: ("not json [").to_owned(),
            },
            Reply::app_state_put,
        )
        .await?;
    assert_wire_error(
        client
            .call_as(&Request::RecentFoldersList {}, Reply::recent_folders_list)
            .await,
        "database",
    );
    assert_wire_error(
        client
            .call_as(
                &Request::RecentFoldersTouch {
                    path: ("new").to_owned(),
                },
                Reply::recent_folders_touch,
            )
            .await,
        "database",
    );
    assert_wire_error(
        client
            .call_as(
                &Request::RecentFoldersRemove {
                    paths: vec!["scratch-1".into()],
                },
                Reply::recent_folders_remove,
            )
            .await,
        "database",
    );
    assert_eq!(
        client
            .call_as(
                &Request::AppStateGet {
                    key: "recent_folders".to_owned()
                },
                Reply::app_state_get
            )
            .await?
            .as_deref(),
        Some("not json [")
    );
    client
        .call_as(
            &Request::AppStatePut {
                key: ("recent_folders").to_owned(),
                value_json: saved.to_owned(),
            },
            Reply::app_state_put,
        )
        .await?;
    assert_eq!(
        client
            .call_as(&Request::RecentFoldersList {}, Reply::recent_folders_list)
            .await?,
        listed
    );

    let second = daemon.paired_client().await?;
    let (_first_send, mut first_events) = client.open_stream(Open::Events).await?;
    let (_second_send, mut second_events) = second.open_stream(Open::Events).await?;
    settings_barrier(&client, &mut first_events, Some(&mut second_events)).await?;
    let entry = "sworm://remote/opaque";
    let touched = client
        .call_as(
            &Request::RecentFoldersTouch {
                path: (entry).to_owned(),
            },
            Reply::recent_folders_touch,
        )
        .await?;
    assert_eq!(touched[0].path, entry);
    assert_eq!(touched[1..], listed[..]);
    assert_recent_event(&mut first_events, &touched).await?;
    assert_recent_event(&mut second_events, &touched).await?;
    let removed = client
        .call_as(
            &Request::RecentFoldersRemove {
                paths: vec![entry.into()],
            },
            Reply::recent_folders_remove,
        )
        .await?;
    assert_eq!(removed, touched[1..]);
    assert_recent_event(&mut first_events, &removed).await?;
    assert_recent_event(&mut second_events, &removed).await?;
    second.close();
    client.close();
    Ok(())
}

async fn shortcuts(client: &RemoteClient, home: &Path) -> Result<()> {
    let missing = client
        .call_as(&Request::ShortcutsGetGlobal {}, Reply::shortcuts_get_global)
        .await?;
    let path = SettingsService::global_shortcuts_path().map_err(anyhow::Error::msg)?;
    assert!(path.starts_with(home));
    assert_eq!(missing.path, path_string(&path));
    assert!(!missing.loaded);
    assert_eq!(missing.value, serde_json::json!({}));
    assert!(!path.exists());

    let created = client
        .call_as(
            &Request::ShortcutsCreateGlobalFile {},
            Reply::shortcuts_create_global_file,
        )
        .await?;
    assert_eq!(created.path, path_string(&path));
    let template = fs::read_to_string(&path)?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&template)?,
        serde_json::json!({"version":1,"bindings":[],"unboundCommands":[]})
    );
    let loaded = client
        .call_as(&Request::ShortcutsGetGlobal {}, Reply::shortcuts_get_global)
        .await?;
    assert!(loaded.loaded);
    assert_eq!(
        loaded.value,
        serde_json::from_str::<serde_json::Value>(&template)?
    );
    assert_eq!(
        client
            .call_as(
                &Request::ShortcutsCreateGlobalFile {},
                Reply::shortcuts_create_global_file
            )
            .await?
            .path,
        path_string(&path)
    );
    assert_eq!(fs::read_to_string(&path)?, template);

    let value = serde_json::json!({"version":1,"bindings":[{"command":"toggle-command-palette","key":"Ctrl+P"}],"unboundCommands":[]});
    let set = client
        .call_as(
            &Request::ShortcutsSetGlobal {
                value: value.clone(),
            },
            Reply::shortcuts_set_global,
        )
        .await?;
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
            .call_as(
                &Request::ShortcutsCreateGlobalFile {},
                Reply::shortcuts_create_global_file
            )
            .await?
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
            .call_as(&Request::ShortcutsGetGlobal {}, Reply::shortcuts_get_global)
            .await?
            .value,
        value
    );
    assert!(matches!(
        client
            .call_as(
                &Request::ShortcutsSetGlobal {
                    value: serde_json::json!([])
                },
                Reply::shortcuts_set_global
            )
            .await,
        Err(RemoteError::Wire(WireError::InvalidArgument { .. }))
    ));
    assert_eq!(fs::read_to_string(&path)?, on_disk);
    assert_eq!(
        client
            .call_as(&Request::ShortcutsGetGlobal {}, Reply::shortcuts_get_global)
            .await?
            .value,
        value
    );
    Ok(())
}

async fn catalogs_and_providers(client: &RemoteClient, home: &Path) -> Result<()> {
    let catalog = client
        .call_as(&Request::BuiltinsGetCatalog {}, Reply::builtins_get_catalog)
        .await?;
    assert!(catalog
        .runtime
        .languages
        .iter()
        .any(|language| language.id == "nix"));
    assert!(catalog.settings.pages.iter().any(|page| page
        .server_definition_ids
        .iter()
        .any(|id| id == "dev.sworm.nix::nil")));

    let binary = home.join("test-omp");
    fs::write(&binary, "#!/bin/sh\nprintf 'test-omp 1.0\\n'\n")?;
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
        .call_as(
            &Request::SettingsPatchGlobalSection {
                input: PatchSettingsSectionInput {
                    section: "providers".into(),
                    value: config,
                },
            },
            Reply::settings_patch_global_section,
        )
        .await?;
    let statuses = client
        .call_as(&Request::ProviderList {}, Reply::provider_list)
        .await?;
    let omp = statuses
        .iter()
        .find(|status| status.id == ProviderId::Omp)
        .context("missing OMP provider")?;
    assert!(matches!(omp.status, ProviderConnectionStatus::Connected));
    assert_eq!(omp.version.as_deref(), Some("test-omp 1.0"));
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
        .call_as(&Request::ActivityMapGet {}, Reply::activity_map_get)
        .await?
        .is_empty());
    let session_dir = home.join(".omp/agent/sessions/-project");
    fs::create_dir_all(&session_dir)?;
    let stem = "2026-01-01T00-00-00-000Z_activity";
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
        .call_as(&Request::ActivityMapRefresh {}, Reply::activity_map_refresh)
        .await?;
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].path, path_string(&project));
    assert_eq!(projects[0].name, "project");
    assert!(projects[0].path_exists);
    assert_eq!(projects[0].providers.len(), 1);
    assert_eq!(projects[0].providers[0].provider_id, "omp");

    let target = client
        .call_as(
            &Request::OmpResolveUri {
                uri: "local://probe.md".into(),
                cwd: Some(path_string(&project)),
            },
            Reply::omp_resolve_uri,
        )
        .await?;
    assert_eq!(target.path, path_string(&artifact));
    assert!(!target.is_dir);
    assert!(matches!(
        client
            .call_as(
                &Request::OmpResolveUri {
                    uri: "unsupported://probe".into(),
                    cwd: Some(path_string(&project)),
                },
                Reply::omp_resolve_uri
            )
            .await,
        Err(RemoteError::Wire(WireError::InvalidArgument { .. }))
    ));
    fs::remove_file(jsonl)?;
    assert_eq!(
        client
            .call_as(&Request::ActivityMapGet {}, Reply::activity_map_get)
            .await?[0]
            .path,
        path_string(&project)
    );
    assert!(client
        .call_as(&Request::ActivityMapRefresh {}, Reply::activity_map_refresh)
        .await?
        .is_empty());

    let root = client
        .call_as(
            &Request::FolderPathRoot {
                path: path_string(home),
            },
            Reply::folder_path_root,
        )
        .await?;
    assert!(matches!(root.kind, PathRootKind::Home));
    assert_eq!(root.path, path_string(&fs::canonicalize(home)?));
    assert_wire_error(
        client
            .call_as(
                &Request::FolderPathRoot {
                    path: path_string(&home.join("missing-folder")),
                },
                Reply::folder_path_root,
            )
            .await,
        "not_found",
    );
    Ok(())
}

#[test]
fn workbench_operations_over_paired_quic() -> Result<()> {
    if !support::test_support::isolated("workbench_operations_over_paired_quic") {
        return Ok(());
    }
    let home = std::path::PathBuf::from(std::env::var_os("HOME").context("missing HOME")?);
    for path in [
        SettingsService::global_settings_path(),
        SettingsService::global_shortcuts_path(),
    ] {
        let path = path.map_err(anyhow::Error::msg)?;
        if !path.starts_with(home.as_path()) {
            bail!("global file escaped scratch HOME: {}", path.display());
        }
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let mut daemon = Daemon::start(None).await?;
        kv_and_recent(&mut daemon).await?;
        let client = daemon.paired_client().await?;
        shortcuts(&client, home.as_path()).await?;
        catalogs_and_providers(&client, home.as_path()).await?;
        activity_and_root(&client, home.as_path()).await?;
        client.close();
        daemon.handle.shutdown().await;
        Ok(())
    })
}

#[test]
fn daemon_persists_presentation_sections_but_rejects_remotes() -> Result<()> {
    if !support::test_support::isolated("daemon_persists_presentation_sections_but_rejects_remotes")
    {
        return Ok(());
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(settings_scenarios())
}

async fn settings_scenarios() -> Result<()> {
    let daemon = Daemon::start(None).await?;
    let client = daemon.paired_client().await?;
    let path = SettingsService::global_settings_path().map_err(anyhow::Error::msg)?;
    assert!(!path.exists(), "scratch global file must start absent");

    let missing = client
        .call_as(
            &Request::SettingsGetGlobalLayer {},
            Reply::settings_get_global_layer,
        )
        .await?;
    assert_eq!(Path::new(&missing.path), path);
    assert!(!missing.loaded);
    assert_eq!(missing.value, serde_json::json!({}));
    assert!(missing.diagnostics.is_empty());
    assert!(!path.exists(), "reading a missing layer must not create it");

    let created = client
        .call_as(
            &Request::SettingsCreateGlobalFile {},
            Reply::settings_create_global_file,
        )
        .await?;
    assert_eq!(Path::new(&created.path), path);
    assert_eq!(fs::read_to_string(&path)?, "{\n}\n");
    let existing = "{\n  \"nix\": { \"eval_timeout_secs\": 41 }\n}\n";
    fs::write(&path, existing)?;
    let again = client
        .call_as(
            &Request::SettingsCreateGlobalFile {},
            Reply::settings_create_global_file,
        )
        .await?;
    assert_eq!(again.path, created.path);
    assert_eq!(fs::read_to_string(&path)?, existing);
    let layer = client
        .call_as(
            &Request::SettingsGetGlobalLayer {},
            Reply::settings_get_global_layer,
        )
        .await?;
    assert!(layer.loaded);
    assert_eq!(
        layer.value,
        serde_json::json!({"nix":{"eval_timeout_secs":41}})
    );
    assert!(layer.diagnostics.is_empty());

    let (_events_send, mut events) = client.open_stream(Open::Events).await?;
    let mut generation = settings_barrier(&client, &mut events, None).await?;

    let window = WindowSettings {
        theme: "test-theme".to_owned(),
        external_folder_open_mode: ExternalFolderOpenMode::FocusedWindow,
        external_file_open_mode: ExternalFileOpenMode::NewWindow,
        tab_beam_position: TabBeamPosition::Bottom,
    };
    let set_window = client
        .call_as(
            &Request::SettingsSetWindow {
                settings: window.clone(),
            },
            Reply::settings_set_window,
        )
        .await?;
    assert_eq!(set_window, window);
    generation = next_global_settings_event(&mut events, generation).await?;
    assert_eq!(daemon_effective(&client).await?.window, window);
    assert_eq!(
        disk_settings(&path)?["window"],
        serde_json::to_value(&window)?
    );

    let terminal = TerminalSettings {
        font_family: "Iosevka".to_owned(),
        font_size: 17,
    };
    let set_terminal = client
        .call_as(
            &Request::SettingsSetTerminal {
                settings: terminal.clone(),
            },
            Reply::settings_set_terminal,
        )
        .await?;
    assert_eq!(set_terminal, terminal);
    generation = next_global_settings_event(&mut events, generation).await?;
    assert_eq!(daemon_effective(&client).await?.terminal, terminal);
    assert_eq!(
        disk_settings(&path)?["terminal"],
        serde_json::to_value(&terminal)?
    );

    let patched_window = serde_json::json!({
        "theme": "patched-theme",
        "external_folder_open_mode": "new_window",
        "external_file_open_mode": "focused_window",
        "tab_beam_position": "top"
    });
    let result = client
        .call_as(
            &Request::SettingsPatchGlobalSection {
                input: PatchSettingsSectionInput {
                    section: "window".to_owned(),
                    value: patched_window.clone(),
                },
            },
            Reply::settings_patch_global_section,
        )
        .await?;
    assert_eq!(result.value["window"], patched_window);
    generation = next_global_settings_event(&mut events, generation).await?;
    assert_eq!(
        serde_json::to_value(&daemon_effective(&client).await?.window)?,
        patched_window
    );
    assert_eq!(disk_settings(&path)?["window"], patched_window);

    let patched_terminal = serde_json::json!({"font_family":"Monaspace", "font_size":19});
    let result = client
        .call_as(
            &Request::SettingsPatchGlobalSection {
                input: PatchSettingsSectionInput {
                    section: "terminal".to_owned(),
                    value: patched_terminal.clone(),
                },
            },
            Reply::settings_patch_global_section,
        )
        .await?;
    assert_eq!(result.value["terminal"], patched_terminal);
    next_global_settings_event(&mut events, generation).await?;
    let effective = daemon_effective(&client).await?;
    assert_eq!(serde_json::to_value(&effective.window)?, patched_window);
    assert_eq!(serde_json::to_value(&effective.terminal)?, patched_terminal);
    assert_eq!(effective.nix.eval_timeout_secs, 41);
    assert!(!effective.explorer.compact_folders);
    let disk_before_rejection = fs::read(&path)?;
    let disk = disk_settings(&path)?;
    assert_eq!(disk["window"], patched_window);
    assert_eq!(disk["terminal"], patched_terminal);
    assert_eq!(disk["nix"]["eval_timeout_secs"], 41);

    let rejection = client
        .call_as(
            &Request::SettingsPatchGlobalSection {
                input: PatchSettingsSectionInput {
                    section: "remotes".to_owned(),
                    value: serde_json::json!({
                        "probe": {"address":"127.0.0.1:1", "fingerprint":"SHA256:probe"}
                    }),
                },
            },
            Reply::settings_patch_global_section,
        )
        .await;
    assert!(matches!(
        rejection,
        Err(RemoteError::Wire(WireError::InvalidArgument { .. }))
    ));
    assert_eq!(
        fs::read(&path)?,
        disk_before_rejection,
        "rejection changed disk"
    );
    assert_eq!(daemon_effective(&client).await?, effective);

    client.close();
    daemon.handle.shutdown().await;
    Ok(())
}

fn disk_settings(path: &Path) -> Result<serde_json::Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

async fn next_global_settings_event(recv: &mut quinn::RecvStream, after: u64) -> Result<u64> {
    timeout(Duration::from_secs(5), async {
        loop {
            if let HostEventWire::SettingsChanged(event) = read_frame::<HostEventWire>(recv).await?
            {
                if event.layer == SettingsLayerKind::Global && event.generation > after {
                    assert_eq!(event.folder_path, None);
                    assert!(event.diagnostics.is_empty());
                    return Ok::<u64, anyhow::Error>(event.generation);
                }
            }
        }
    })
    .await
    .context("no global settings event after patch")?
}

async fn daemon_effective(client: &RemoteClient) -> Result<EffectiveSettings> {
    Ok(client
        .call_as(
            &Request::SettingsGetEffective { folder_path: None },
            Reply::settings_get_effective,
        )
        .await?
        .settings)
}

async fn settings_barrier(
    client: &RemoteClient,
    first: &mut quinn::RecvStream,
    second: Option<&mut quinn::RecvStream>,
) -> Result<u64> {
    timeout(WAIT, async {
        // Never cancel a partial frame read between readiness patches.
        let first = next_global_settings_event(first, 0);
        let second = async {
            match second {
                Some(recv) => next_global_settings_event(recv, 0).await,
                None => Ok(0),
            }
        };
        tokio::pin!(first, second);
        let mut observed = [false, false];
        let mut generation = 0;
        let mut attempts = tokio::time::interval(Duration::from_millis(100));
        while !observed.iter().all(|ready| *ready) {
            tokio::select! {
                result = &mut first, if !observed[0] => {
                    generation = generation.max(result?);
                    observed[0] = true;
                }
                result = &mut second, if !observed[1] => {
                    generation = generation.max(result?);
                    observed[1] = true;
                }
                _ = attempts.tick() => {
                    let ready = client.call_as(&Request::SettingsPatchGlobalSection {
                        input: PatchSettingsSectionInput {
                            section: "explorer".into(),
                            value: serde_json::json!({"compact_folders": false}),
                        },
                    }, Reply::settings_patch_global_section).await?;
                    assert_eq!(ready.value["explorer"]["compact_folders"], false);
                }
            }
        }
        Ok::<u64, anyhow::Error>(generation)
    })
    .await
    .context("event streams did not become ready")?
}
