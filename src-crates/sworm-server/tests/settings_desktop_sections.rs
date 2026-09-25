//! Daemon global settings resolve through this process's HOME/XDG, not
//! `ServeOptions`. This dedicated test binary forces scratch paths before
//! constructing a runtime so presentation-setting writes cannot touch user files.

use anyhow::{bail, Context, Result};
use std::{fs, path::Path, sync::LazyLock, time::Duration};
use sworm_core::services::settings::SettingsService;
use sworm_protocol::{
    rpc::{HostEventFrame, HostEventWire, Open, Request, WireError},
    settings::{
        EffectiveSettings, EffectiveSettingsInput, ExternalFileOpenMode, ExternalFolderOpenMode,
        PatchSettingsSectionInput, SettingsLayerKind, TabBeamPosition, TerminalSettings,
        WindowSettings,
    },
};
use sworm_remote::{wire::read_frame, Identity, RemoteClient, RemoteError};
use sworm_server::{auth, serve, ServeOptions, ServerHandle};
use tempfile::TempDir;
use tokio::time::timeout;

/// Initialize process-wide paths before Tokio or the daemon can resolve them.
static SCRATCH_HOME: LazyLock<TempDir> = LazyLock::new(|| {
    let home = tempfile::tempdir().expect("scratch home");
    std::env::set_var("HOME", home.path());
    std::env::set_var("XDG_CONFIG_HOME", home.path().join("config"));
    std::env::set_var("XDG_DATA_HOME", home.path().join("data"));
    std::env::set_var("GIT_CONFIG_GLOBAL", home.path().join("gitconfig"));
    std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
    home
});

struct Daemon {
    config: TempDir,
    _data: TempDir,
    endpoint: quinn::Endpoint,
    handle: ServerHandle,
    client_identity: Identity,
}

impl Daemon {
    async fn start() -> Result<Self> {
        let home = &*SCRATCH_HOME;
        let settings_path = SettingsService::global_settings_path().map_err(anyhow::Error::msg)?;
        let config_root = std::env::var_os("XDG_CONFIG_HOME").context("missing XDG_CONFIG_HOME")?;
        let data_root = std::env::var_os("XDG_DATA_HOME").context("missing XDG_DATA_HOME")?;
        if !settings_path.starts_with(home.path())
            || !Path::new(&config_root).starts_with(home.path())
            || !Path::new(&data_root).starts_with(home.path())
        {
            bail!("global settings or XDG directories resolved outside scratch home");
        }
        let config = tempfile::tempdir()?;
        let data = tempfile::tempdir()?;
        let client_dir = tempfile::tempdir()?;
        let client_identity = Identity::load_or_generate(client_dir.path(), "client")?;
        let endpoint = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
        let handle = serve(ServeOptions {
            config_dir: config.path().to_path_buf(),
            data_dir: data.path().to_path_buf(),
            listen: Some("127.0.0.1:0".parse()?),
        })
        .await?;
        Ok(Self {
            config,
            _data: data,
            endpoint,
            handle,
            client_identity,
        })
    }

    async fn paired_client(&self) -> Result<RemoteClient> {
        let token = auth::write_pairing_token(self.config.path())?;
        let client = RemoteClient::connect(
            &self.endpoint,
            self.handle.local_addr,
            &self.client_identity,
            self.handle.fingerprint,
        )
        .await?;
        client.pair(&token, "test client").await?;
        Ok(client)
    }
}

#[test]
fn daemon_persists_presentation_sections_but_rejects_remotes() -> Result<()> {
    let _home = &*SCRATCH_HOME;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(settings_scenarios())
}

async fn settings_scenarios() -> Result<()> {
    let daemon = Daemon::start().await?;
    let client = daemon.paired_client().await?;
    let path = SettingsService::global_settings_path().map_err(anyhow::Error::msg)?;
    assert!(!path.exists(), "scratch global file must start absent");

    let missing = client
        .call(&Request::SettingsGetGlobalLayer {})
        .await?
        .settings_get_global_layer()
        .map_err(RemoteError::Wire)?;
    assert_eq!(Path::new(&missing.path), path);
    assert!(!missing.loaded);
    assert_eq!(missing.value, serde_json::json!({}));
    assert!(missing.diagnostics.is_empty());
    assert!(!path.exists(), "reading a missing layer must not create it");

    let created = client
        .call(&Request::SettingsCreateGlobalFile {})
        .await?
        .settings_create_global_file()
        .map_err(RemoteError::Wire)?;
    assert_eq!(Path::new(&created.path), path);
    assert_eq!(fs::read_to_string(&path)?, "{\n}\n");
    let existing = "{\n  \"nix\": { \"eval_timeout_secs\": 41 }\n}\n";
    fs::write(&path, existing)?;
    let again = client
        .call(&Request::SettingsCreateGlobalFile {})
        .await?
        .settings_create_global_file()
        .map_err(RemoteError::Wire)?;
    assert_eq!(again.path, created.path);
    assert_eq!(fs::read_to_string(&path)?, existing);
    let layer = client
        .call(&Request::SettingsGetGlobalLayer {})
        .await?
        .settings_get_global_layer()
        .map_err(RemoteError::Wire)?;
    assert!(layer.loaded);
    assert_eq!(
        layer.value,
        serde_json::json!({"nix":{"eval_timeout_secs":41}})
    );
    assert!(layer.diagnostics.is_empty());

    let (_events_send, mut events) = client.open_stream(Open::Events).await?;
    // An Open::Events stream may not have been installed by the time this
    // request starts. Retry the settings round trip until an event arrives,
    // keeping the frame read alive across ticks (never cancel a partial frame).
    let mut generation = timeout(Duration::from_secs(5), async {
        let mut attempts = tokio::time::interval(Duration::from_millis(100));
        let event = next_global_settings_event(&mut events, 0);
        tokio::pin!(event);
        loop {
            tokio::select! {
                result = &mut event => return result,
                _ = attempts.tick() => {
                    let ready = client
                        .call(&Request::SettingsPatchGlobalSection {
                            input: PatchSettingsSectionInput {
                                section: "explorer".to_owned(),
                                value: serde_json::json!({"compact_folders": false}),
                            },
                        })
                        .await?
                        .settings_patch_global_section()
                        .map_err(RemoteError::Wire)?;
                    assert_eq!(ready.value["explorer"]["compact_folders"], false);
                }
            }
        }
    })
    .await
    .context("event stream did not receive the readiness patch")??;

    let window = WindowSettings {
        theme: "phase1-theme".to_owned(),
        external_folder_open_mode: ExternalFolderOpenMode::FocusedWindow,
        external_file_open_mode: ExternalFileOpenMode::NewWindow,
        tab_beam_position: TabBeamPosition::Bottom,
    };
    let set_window = client
        .call(&Request::SettingsSetWindow {
            settings: window.clone(),
        })
        .await?
        .settings_set_window()
        .map_err(RemoteError::Wire)?;
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
        .call(&Request::SettingsSetTerminal {
            settings: terminal.clone(),
        })
        .await?
        .settings_set_terminal()
        .map_err(RemoteError::Wire)?;
    assert_eq!(set_terminal, terminal);
    generation = next_global_settings_event(&mut events, generation).await?;
    assert_eq!(daemon_effective(&client).await?.terminal, terminal);
    assert_eq!(
        disk_settings(&path)?["terminal"],
        serde_json::to_value(&terminal)?
    );

    let patched_window = serde_json::json!({
        "theme": "phase1-patched",
        "external_folder_open_mode": "new_window",
        "external_file_open_mode": "focused_window",
        "tab_beam_position": "top"
    });
    let result = client
        .call(&Request::SettingsPatchGlobalSection {
            input: PatchSettingsSectionInput {
                section: "window".to_owned(),
                value: patched_window.clone(),
            },
        })
        .await?
        .settings_patch_global_section()
        .map_err(RemoteError::Wire)?;
    assert_eq!(result.value["window"], patched_window);
    generation = next_global_settings_event(&mut events, generation).await?;
    assert_eq!(
        serde_json::to_value(&daemon_effective(&client).await?.window)?,
        patched_window
    );
    assert_eq!(disk_settings(&path)?["window"], patched_window);

    let patched_terminal = serde_json::json!({"font_family":"Monaspace", "font_size":19});
    let result = client
        .call(&Request::SettingsPatchGlobalSection {
            input: PatchSettingsSectionInput {
                section: "terminal".to_owned(),
                value: patched_terminal.clone(),
            },
        })
        .await?
        .settings_patch_global_section()
        .map_err(RemoteError::Wire)?;
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
        .call(&Request::SettingsPatchGlobalSection {
            input: PatchSettingsSectionInput {
                section: "remotes".to_owned(),
                value: serde_json::json!({
                    "probe": {"address":"127.0.0.1:1", "fingerprint":"SHA256:probe"}
                }),
            },
        })
        .await;
    assert!(matches!(
        rejection,
        Err(RemoteError::Wire(WireError::InvalidArgument { message }))
            if message == "remotes settings are desktop-only"
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
            if let HostEventWire::SettingsChanged(event) =
                read_frame::<HostEventFrame>(recv).await?.0
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
        .call(&Request::SettingsGetEffective {
            input: EffectiveSettingsInput { folder_path: None },
        })
        .await?
        .settings_get_effective()
        .map_err(RemoteError::Wire)?
        .settings)
}
