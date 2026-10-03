use crate::app_state::AppState;
use parking_lot::Mutex;
use percent_encoding::percent_decode_str;
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager, WebviewWindow};
use tauri_plugin_deep_link::DeepLinkExt;

/// The lone URL argument the deep-link plugin claims (it ignores URLs mixed
/// with other arguments).
pub(crate) fn deep_link_arg(argv: &[String]) -> Option<tauri::Url> {
    let [_, arg] = argv else { return None };
    tauri::Url::parse(arg)
        .ok()
        .filter(|url| matches!(url.scheme(), "sworm-pair" | "sworm"))
}

/// Resolve existing launch paths lexically, preserving symlink path forms.
/// `sworm://` remote workspaces name no local file; they are only decoded.
pub(crate) fn launch_path_args(argv: &[String], cwd: Option<&Path>) -> Vec<String> {
    argv.iter()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .filter_map(|arg| {
            if arg.starts_with("sworm://") {
                return parse_workspace_link(arg);
            }
            let path = match tauri::Url::parse(arg) {
                Ok(url) if url.scheme() == "file" => {
                    if url.query().is_some() || url.fragment().is_some() {
                        return None;
                    }
                    url.to_file_path().ok()?
                }
                _ => PathBuf::from(arg),
            };
            let path = if path.is_absolute() {
                path
            } else {
                cwd?.join(path)
            };
            let path = sworm_core::services::folders::normalize_absolute_path(&path);
            (path.is_file() || path.is_dir()).then(|| path.to_string_lossy().into_owned())
        })
        .collect()
}

/// Decode an external `sworm://server/path` link exactly once into the opaque
/// internal workspace path; downstream code never decodes it again.
pub(crate) fn parse_workspace_link(link: &str) -> Option<String> {
    let rest = link.strip_prefix("sworm://")?;
    let server = &rest[..rest.find('/').unwrap_or(rest.len())];
    // Only a bare server id: userinfo, ports, queries, and fragments would make
    // the target ambiguous.
    if server.is_empty()
        || !server
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        || rest
            .bytes()
            .any(|b| b <= b' ' || matches!(b, 0x7f | b'?' | b'#'))
    {
        return None;
    }
    let url = tauri::Url::parse(link).ok()?;
    let encoded = if url.path().is_empty() {
        "/"
    } else {
        url.path()
    };
    // percent_decode_str passes malformed escapes through; reject them instead.
    if encoded.split('%').skip(1).any(|escape| {
        !escape
            .get(..2)
            .is_some_and(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()))
    }) {
        return None;
    }
    let path = percent_decode_str(encoded).decode_utf8().ok()?;
    (!path.contains('\0')).then(|| format!("sworm://{server}{path}"))
}

#[derive(Default)]
pub struct DeepLinks(Mutex<PendingLinks>);

#[derive(Default)]
struct PendingLinks {
    window: Option<String>,
    urls: Vec<String>,
}

impl PendingLinks {
    fn take(&mut self, window: &str) -> Vec<String> {
        if self.window.as_deref() != Some(window) {
            return Vec::new();
        }
        std::mem::take(&mut self.urls)
    }
}

pub fn init(app: &tauri::AppHandle) -> Result<(), tauri_plugin_deep_link::Error> {
    if let Some(urls) = app.deep_link().get_current()? {
        open(app, urls);
    }
    let handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        open(&handle, event.urls());
    });
    Ok(())
}

/// Workspace links honor the external folder-open setting like any launch
/// path; only pairing links (which carry secrets) queue for one workbench.
fn open(app: &tauri::AppHandle, urls: Vec<tauri::Url>) {
    let mut pairing: Vec<String> = Vec::new();
    for url in urls {
        match url.scheme() {
            "sworm" => match parse_workspace_link(url.as_str()) {
                Some(path) => app.state::<AppState>().windows.route_open_path(app, &path),
                None => tracing::warn!("Ignoring malformed workspace link: {url}"),
            },
            "sworm-pair" => pairing.push(url.into()),
            _ => {}
        }
    }
    if pairing.is_empty() {
        return;
    }
    {
        let links = app.state::<DeepLinks>();
        let mut pending = links.0.lock();
        pending.urls.extend(pairing);
        pending.window = app.state::<AppState>().windows.get_focused_window_label();
    }
    notify(app);
}

pub fn window_closed(app: &tauri::AppHandle, label: &str) {
    {
        let links = app.state::<DeepLinks>();
        let mut pending = links.0.lock();
        if pending.window.as_deref() == Some(label) {
            pending.window = None;
        }
    }
    notify(app);
}

/// Signal only the chosen workbench. URLs (including pairing secrets) stay in
/// the queue until that window claims them; late listeners drain on mount too.
pub fn notify(app: &tauri::AppHandle) {
    let links = app.state::<DeepLinks>();
    let window = {
        let mut pending = links.0.lock();
        if pending.urls.is_empty() {
            return;
        }
        if pending
            .window
            .as_deref()
            .and_then(|label| app.get_webview_window(label))
            .is_none()
        {
            pending.window = app.state::<AppState>().windows.get_focused_window_label();
        }
        pending.window.clone()
    };
    if let Some(label) = window {
        if let Err(error) = app.emit_to(label.as_str(), "deep-link-open", ()) {
            tracing::warn!("Failed to signal queued deep link: {error}");
        }
    }
}

#[tauri::command]
pub fn deep_link_take(window: WebviewWindow) -> Vec<String> {
    window
        .app_handle()
        .state::<DeepLinks>()
        .0
        .lock()
        .take(window.label())
}

#[cfg(test)]
mod tests {
    use super::{launch_path_args, parse_workspace_link, PendingLinks};

    #[test]
    fn only_target_window_can_claim_each_delivery_once() {
        let link = "sworm-pair://pair?token=secret";
        let mut pending = PendingLinks {
            window: Some("workbench-a".into()),
            urls: vec![link.into()],
        };
        assert!(pending.take("workbench-b").is_empty());
        assert_eq!(pending.take("workbench-a"), vec![link]);
        assert!(pending.take("workbench-a").is_empty());
        pending.urls.push(link.into());
        assert_eq!(pending.take("workbench-a"), vec![link]);
    }

    #[test]
    fn launch_path_args_ignores_argv0_and_flags_and_uses_cwd_for_relative_paths() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("cwd");
        let project = root.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&project).unwrap();

        // argv[0] is the binary; leading '-' flags must be skipped.
        let argv = vec!["sworm".into(), "--some-flag".into(), "../project".into()];
        let resolved = launch_path_args(&argv, Some(cwd.as_path()));

        assert_eq!(resolved, vec![project.to_string_lossy().into_owned()]);
    }

    #[test]
    fn launch_path_args_returns_empty_for_missing_path() {
        let argv = vec!["sworm".into(), "/nonexistent/path/xyz".into()];
        assert!(launch_path_args(&argv, None).is_empty());
    }

    /// A lone URL argument belongs to the deep-link plugin; several arguments
    /// reach argv routing, which must decode remote workspaces the same way.
    #[test]
    fn launch_path_args_decodes_remote_workspace_uris() {
        let argv = vec![
            "sworm".into(),
            "sworm://homelab/home/me/my%20project".into(),
            "sworm://homelab/home/me/%ZZ".into(),
        ];
        assert_eq!(
            launch_path_args(&argv, None),
            vec!["sworm://homelab/home/me/my project".to_owned()]
        );
    }

    #[test]
    fn workspace_links_decode_once_and_reject_ambiguous_forms() {
        for (link, expected) in [
            (
                "sworm://BuildBox/my%20repo/%E6%96%87%E4%BB%B6",
                "sworm://BuildBox/my repo/文件",
            ),
            (
                "sworm://BuildBox/literal%2520name",
                "sworm://BuildBox/literal%20name",
            ),
            ("sworm://BuildBox/a%23b%3Fc", "sworm://BuildBox/a#b?c"),
            ("sworm://BuildBox", "sworm://BuildBox/"),
        ] {
            assert_eq!(
                parse_workspace_link(link).as_deref(),
                Some(expected),
                "{link}"
            );
        }
        for link in [
            "sworm://user@host/repo",
            "sworm://host:7420/repo",
            "sworm:///repo",
            "sworm://host/repo?query",
            "sworm://host/repo#fragment",
            "sworm://host/repo?",
            "sworm://host/repo#",
            "sworm://host/%ZZ",
            "sworm://host/%C0%AF",
            "sworm://host/%00",
            "sworm://host/repo\n",
            "sworm://host/my repo",
        ] {
            assert_eq!(parse_workspace_link(link), None, "{link:?}");
        }
    }

    #[test]
    fn launch_path_args_decodes_local_file_uris_and_rejects_remote_hosts() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project #1 %");
        std::fs::create_dir(&project).unwrap();
        let uri = tauri::Url::from_directory_path(&project).unwrap();
        let expected = project.to_string_lossy().into_owned();
        for uri in [
            uri.to_string(),
            uri.as_str().replacen("file:///", "file://localhost/", 1),
        ] {
            assert_eq!(
                launch_path_args(&["sworm".into(), uri], None),
                vec![expected.clone()]
            );
        }
        for uri in [
            uri.as_str().replacen("file:///", "file://remote/", 1),
            format!("{uri}?query"),
            format!("{uri}#fragment"),
        ] {
            assert!(launch_path_args(&["sworm".into(), uri], None).is_empty());
        }
    }

    #[cfg(unix)]
    #[test]
    fn launch_path_args_preserves_symlink_path_form() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real-project");
        let link = root.path().join("project-link");
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let argv = vec!["sworm".into(), link.to_string_lossy().into_owned()];
        let resolved = launch_path_args(&argv, None);

        assert_eq!(resolved, vec![link.to_string_lossy().into_owned()]);
    }
}
