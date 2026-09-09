use crate::errors::ApiError;
use crate::host::Host;
use crate::services::folders::{folder_name, resolve_folder};
use sworm_protocol::folder::{FolderEntry, FolderInfo};

impl Host {
    /// Canonicalize a folder path and return its display name.
    pub async fn folder_resolve(&self, path: String) -> Result<FolderInfo, ApiError> {
        let folder = resolve_folder(&path)?;
        Ok(FolderInfo {
            name: folder_name(&folder),
            path: folder.to_string_lossy().into_owned(),
        })
    }

    /// Immediate children of a canonicalized directory; directories first, then
    /// case-insensitive by name.
    pub async fn folder_list_entries(
        &self,
        path: String,
        show_hidden: bool,
    ) -> Result<Vec<FolderEntry>, ApiError> {
        list_entries(&path, show_hidden)
    }
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
