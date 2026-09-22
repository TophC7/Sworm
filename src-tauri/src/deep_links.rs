use crate::app_state::AppState;
use crate::commands::app::parse_workspace_link;
use parking_lot::Mutex;
use tauri::{Emitter, Manager, WebviewWindow};
use tauri_plugin_deep_link::DeepLinkExt;

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
    use super::PendingLinks;

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
}
