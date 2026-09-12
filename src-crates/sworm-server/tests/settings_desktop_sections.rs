//! Settings patches land in the daemon's *global* settings file, and that path
//! is resolved from this process's environment, not from `ServeOptions`. These
//! tests therefore live in their own test binary: it repoints `HOME` and the
//! XDG directories at scratch space before any daemon starts, so even a
//! regression that lets a desktop-only patch through cannot reach the
//! developer's real `~/.config/sworm`.

use anyhow::{bail, Result};
use std::sync::LazyLock;
use sworm_core::services::settings::SettingsService;
use sworm_protocol::{
    rpc::{Request, WireError},
    settings::{
        EffectiveSettings, EffectiveSettingsInput, PatchSettingsSectionInput, TabBeamPosition,
        DESKTOP_SECTIONS,
    },
};
use sworm_remote::{Identity, RemoteClient, RemoteError};
use sworm_server::{auth, serve, ServeOptions, ServerHandle};
use tempfile::TempDir;

/// Scratch `HOME`/XDG for the whole binary. Every daemon here is started
/// through `Daemon::start`, which touches this first, so the one-time write
/// happens-before any settings path is resolved in this process.
static SCRATCH_HOME: LazyLock<TempDir> = LazyLock::new(|| {
    let home = tempfile::tempdir().expect("scratch home");
    std::env::set_var("HOME", home.path());
    std::env::set_var("XDG_CONFIG_HOME", home.path().join("config"));
    std::env::set_var("XDG_DATA_HOME", home.path().join("data"));
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
        if !settings_path.starts_with(home.path()) {
            bail!("global settings resolved outside the scratch home: {settings_path:?}");
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

#[tokio::test(flavor = "multi_thread")]
async fn daemon_rejects_desktop_section_patch() -> Result<()> {
    let daemon = Daemon::start().await?;
    let client = daemon.paired_client().await?;
    let before = daemon_effective(&client).await?;

    // window/terminal/remotes describe the desktop window, not the machine that
    // runs the folder. The daemon must refuse them before touching its own
    // settings file, or a remote workspace would silently retheme the server.
    // Each probe differs from what the daemon currently resolves, so a patch
    // that landed would show up in its effective settings.
    for section in DESKTOP_SECTIONS {
        let flipped_beam = match before.window.tab_beam_position {
            TabBeamPosition::Top => "bottom",
            TabBeamPosition::Bottom => "top",
        };
        let value = match *section {
            "window" => serde_json::json!({ "tab_beam_position": flipped_beam }),
            "terminal" => {
                serde_json::json!({ "font_size": before.terminal.font_size.saturating_add(1) })
            }
            "remotes" => serde_json::json!({
                "probe": { "address": "127.0.0.1:1", "fingerprint": "SHA256:probe" }
            }),
            other => bail!("desktop section {other} has no probe value in this test"),
        };
        let result = client
            .call(&Request::SettingsPatchGlobalSection {
                input: PatchSettingsSectionInput {
                    section: (*section).to_owned(),
                    value,
                },
            })
            .await;
        if !matches!(
            result,
            Err(RemoteError::Wire(WireError::InvalidArgument { .. }))
        ) {
            bail!("{section} patch was not rejected: {result:?}");
        }
    }

    assert_eq!(
        daemon_effective(&client).await?,
        before,
        "a rejected desktop patch still changed the daemon's settings"
    );

    // Control: the same request shape does land for a host section, so the
    // comparison above measures refusal rather than an inert code path.
    client
        .call(&Request::SettingsPatchGlobalSection {
            input: PatchSettingsSectionInput {
                section: "explorer".to_owned(),
                value: serde_json::json!({ "compact_folders": true }),
            },
        })
        .await?
        .settings_patch_global_section()
        .map_err(RemoteError::Wire)?;
    let after = daemon_effective(&client).await?;
    assert!(after.explorer.compact_folders);
    assert_eq!(after.window, before.window);
    assert_eq!(after.terminal, before.terminal);
    assert_eq!(after.remotes, before.remotes);

    client.close();
    daemon.handle.shutdown().await;
    Ok(())
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
