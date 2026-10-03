mod common;

use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use sworm_core::{errors::ApiError, Host};
use sworm_lib::router::WorkspaceRouter;
use sworm_protocol::settings::PatchSettingsSectionInput;
use sworm_remote::Identity;
use tempfile::tempdir;

/// The retry loop resolves a stop's server by name and reads an unknown name
/// as "run gone". A rename must carry queued stops along, and a removal must
/// wait for them, or the remote process is orphaned.
#[tokio::test(flavor = "multi_thread")]
async fn pending_stops_follow_remote_renames() -> anyhow::Result<()> {
    // This binary owns process-global XDG/HOME for its whole lifetime.
    let temporary = tempdir()?;
    common::isolate(temporary.path())?;

    let fingerprint = Identity::load_or_generate(temporary.path(), "server")?.fingerprint();
    let host = Arc::new(Host::new(
        temporary.path().join("sworm.db"),
        Arc::new(|_| Ok(())),
    )?);
    let router = WorkspaceRouter::new(Arc::clone(&host));
    router
        .settings_patch_global_section(
            None,
            PatchSettingsSectionInput {
                section: "remotes".into(),
                value: json!({
                    "old": {
                        "address": "sworm-unreachable.invalid:7420",
                        "fingerprint": fingerprint.to_string(),
                    }
                }),
            },
        )
        .await?;

    router.queue_stop_for_test("old", "orphan-run");
    router.change_remote("old", Some("new")).await?;
    let expected = HashMap::from([("orphan-run".to_owned(), "new".to_owned())]);
    assert_eq!(router.pending_stops_for_test(), expected);
    assert_eq!(
        WorkspaceRouter::new(Arc::clone(&host)).pending_stops_for_test(),
        expected,
        "the rename must reach the persisted stops"
    );

    let removed = router.change_remote("new", None).await;
    assert!(
        matches!(removed, Err(ApiError::InvalidArgument(_))),
        "removing a remote with a pending stop must be refused, got {removed:?}"
    );
    assert_eq!(router.pending_stops_for_test(), expected);
    Ok(())
}
