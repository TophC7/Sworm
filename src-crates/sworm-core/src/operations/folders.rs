use crate::errors::ApiError;
use crate::events::HostEvent;
use crate::host::Host;
use crate::services::app_state_kv::AppStateKvService;
use crate::services::folders::{find_path_root, folder_name, home_dir, resolve_folder};
use rusqlite::Connection;
use sworm_protocol::folder::{FolderEntry, FolderInfo, PathRoot};
use sworm_protocol::rpc::RecentFolder;

const RECENT_FOLDERS_KEY: &str = "recent_folders";

impl Host {
    /// Canonicalize a folder path and return its display name.
    pub fn folder_resolve(&self, path: String) -> Result<FolderInfo, ApiError> {
        let folder = resolve_folder(&path)?;
        Ok(FolderInfo {
            name: folder_name(&folder),
            path: folder.to_string_lossy().into_owned(),
        })
    }

    /// The path-root anchor is for browsing, not folder ownership.
    pub fn folder_path_root(&self, path: String) -> Result<PathRoot, ApiError> {
        Ok(find_path_root(&resolve_folder(&path)?))
    }

    /// Canonical `$HOME`, where browsing a host starts.
    pub fn folder_home(&self) -> Result<String, ApiError> {
        let home = home_dir().ok_or_else(|| ApiError::NotFound("HOME is not set".to_owned()))?;
        Ok(resolve_folder(&home.to_string_lossy())?
            .to_string_lossy()
            .into_owned())
    }

    pub fn recent_folders_list(&self) -> Result<Vec<RecentFolder>, ApiError> {
        let db = self.db.read();
        read_recent_folders(db.conn())
    }

    pub fn recent_folders_touch(&self, path: String) -> Result<Vec<RecentFolder>, ApiError> {
        let db = self.db.write();
        let mut folders = read_recent_folders(db.conn())?;
        folders.retain(|folder| folder.path != path);
        folders.insert(
            0,
            RecentFolder {
                path,
                opened_at: chrono::Utc::now().to_rfc3339(),
            },
        );
        folders.truncate(12);
        save_recent_folders(db.conn(), &folders)?;
        self.emit(HostEvent::RecentFoldersChanged(folders.clone()));
        Ok(folders)
    }

    pub fn recent_folders_remove(&self, paths: Vec<String>) -> Result<Vec<RecentFolder>, ApiError> {
        let db = self.db.write();
        let mut folders = read_recent_folders(db.conn())?;
        folders.retain(|folder| !paths.contains(&folder.path));
        save_recent_folders(db.conn(), &folders)?;
        self.emit(HostEvent::RecentFoldersChanged(folders.clone()));
        Ok(folders)
    }

    /// Immediate children of a canonicalized directory; directories first, then
    /// case-insensitive by name.
    pub fn folder_list_entries(
        &self,
        path: String,
        show_hidden: bool,
    ) -> Result<Vec<FolderEntry>, ApiError> {
        list_entries(&path, show_hidden)
    }
}

fn read_recent_folders(conn: &Connection) -> Result<Vec<RecentFolder>, ApiError> {
    AppStateKvService::get(conn, RECENT_FOLDERS_KEY)
        .map_err(ApiError::Database)?
        .map(|json| {
            serde_json::from_str(&json).map_err(|error| ApiError::Database(error.to_string()))
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

fn save_recent_folders(conn: &Connection, folders: &[RecentFolder]) -> Result<(), ApiError> {
    let json =
        serde_json::to_string(folders).map_err(|error| ApiError::Internal(error.to_string()))?;
    AppStateKvService::put(conn, RECENT_FOLDERS_KEY, &json).map_err(ApiError::Database)
}

fn list_entries(path: &str, show_hidden: bool) -> Result<Vec<FolderEntry>, ApiError> {
    let directory = resolve_folder(path)?;
    let mut entries = Vec::new();

    for entry in std::fs::read_dir(&directory)? {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }

        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let is_dir = if file_type.is_symlink() {
            entry.path().is_dir()
        } else {
            file_type.is_dir()
        };
        let entry_path = entry.path();
        let sort_name = name.to_lowercase();
        entries.push((
            sort_name,
            FolderEntry {
                name,
                path: entry_path.to_string_lossy().into_owned(),
                is_dir,
            },
        ));
    }

    entries.sort_by(|(a_sort, a), (b_sort, b)| {
        (!a.is_dir, a_sort, &a.name).cmp(&(!b.is_dir, b_sort, &b.name))
    });
    Ok(entries.into_iter().map(|(_, entry)| entry).collect())
}

#[cfg(test)]
mod tests {
    use super::list_entries;
    use std::fs;

    #[test]
    fn lists_mixed_entries_dirs_first_and_filters_hidden() {
        let root = std::env::temp_dir().join(format!("sworm-folder-list-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("zeta")).unwrap();
        fs::create_dir(root.join("Alpha")).unwrap();
        fs::create_dir(root.join("alpha")).unwrap();
        fs::create_dir(root.join(".hidden")).unwrap();
        fs::write(root.join("beta.txt"), "file").unwrap();
        fs::write(root.join("Gamma.txt"), "file").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("Alpha"), root.join("linked-alpha")).unwrap();

        let visible = list_entries(root.to_str().unwrap(), false)
            .unwrap()
            .into_iter()
            .map(|entry| (entry.name, entry.is_dir))
            .collect::<Vec<_>>();
        let mut expected = vec![
            ("Alpha".to_owned(), true),
            ("alpha".to_owned(), true),
            ("zeta".to_owned(), true),
            ("beta.txt".to_owned(), false),
            ("Gamma.txt".to_owned(), false),
        ];
        #[cfg(unix)]
        expected.insert(2, ("linked-alpha".to_owned(), true));
        assert_eq!(visible, expected);

        let hidden = list_entries(root.to_str().unwrap(), true).unwrap();
        assert_eq!(
            hidden.first().map(|entry| (&*entry.name, entry.is_dir)),
            Some((".hidden", true))
        );

        fs::remove_dir_all(root).unwrap();
    }
}
