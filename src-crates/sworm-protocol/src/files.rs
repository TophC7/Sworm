use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct FilePasteCollision {
    pub source: String,
    pub destination: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePasteMapping {
    pub source: String,
    pub destination: String,
}

/// One row in a directory listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    /// Display label. A compacted chain carries the whole run, e.g. "lib/utils".
    pub name: String,
    /// Project-relative path with forward slashes. For a compacted chain this
    /// is the deepest directory, which is also the key its children load under.
    pub path: String,
    pub is_dir: bool,
    /// Matched git's ignore rules — rendered dimmed.
    pub ignored: bool,
    /// Matched an `explorer.exclude` glob; only ever true when `show_hidden`.
    pub excluded: bool,
    /// Directories a compacted chain swallowed, excluding `path` itself. The
    /// explorer watches these too: a write inside one changes what the row
    /// should collapse to, and no other listing would report it.
    pub hops: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PathList {
    pub paths: Vec<String>,
    pub truncated: bool,
}

pub const FILES_CHANGED_EVENT: &str = "files-changed";

#[derive(Debug, Clone, Serialize)]
pub struct FilesChangedEvent {
    pub folder_path: String,
    /// Project-relative directories whose contents changed; "" is the root.
    pub dirs: Vec<String>,
}
