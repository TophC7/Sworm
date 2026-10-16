//! Copy selected untracked files without trusting path resolution after checkout creation.

use super::super::{git_command, git_output};
use crate::errors::ApiError;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, CString, OsStr};
use std::fs::{File, Permissions};
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use sworm_protocol::git::GitWorktreeList;

#[derive(Debug)]
pub(super) struct CopySnapshot {
    // Keep the enumerated root alive even if its pathname is subsequently replaced.
    source: Option<File>,
    candidates: Vec<String>,
    warnings: Vec<String>,
}

pub(super) fn snapshot(
    source: &Path,
    inventory: &GitWorktreeList,
    patterns: &BTreeMap<String, bool>,
) -> Result<CopySnapshot, ApiError> {
    let mut snapshot = CopySnapshot {
        source: None,
        candidates: Vec::new(),
        warnings: Vec::new(),
    };
    if patterns.is_empty() {
        return Ok(snapshot);
    }
    let (include, veto) = compile_patterns(patterns)?;
    if include.is_empty() {
        return Ok(snapshot);
    }
    let root = open_root(source).map_err(|error| {
        ApiError::Io(format!(
            "Cannot open copy source {}: {error}",
            source.display()
        ))
    })?;
    let output = git_output(
        &fd_path(&root),
        &["ls-files", "--others", "-z", "--"],
        "enumerate worktree copy candidates",
    )
    .map_err(ApiError::Internal)?;
    let names = utf8_paths(&output).map_err(ApiError::InvalidArgument)?;
    let excluded: Vec<&Path> = inventory
        .worktrees
        .iter()
        .map(|worktree| Path::new(&worktree.path))
        .chain(std::iter::once(Path::new(&inventory.common_dir)))
        .filter_map(|path| path.strip_prefix(source).ok())
        .filter(|path| !path.as_os_str().is_empty())
        .collect();

    for name in names {
        let path = relative_path(name).map_err(|reason| {
            ApiError::InvalidArgument(format!("Invalid worktree copy path {name:?}: {reason}"))
        })?;
        if !matches_path_or_parent(&include, path) || matches_path_or_parent(&veto, path) {
            continue;
        }
        let inspection = if has_git_component(path) {
            Err(CopyError::Skip("Git metadata is never copied"))
        } else if excluded.iter().any(|root| path.starts_with(root)) {
            Err(CopyError::Skip(
                "registered repository or Git metadata directory",
            ))
        } else {
            inspect_source(&root, path)
        };
        match inspection {
            Ok(()) => snapshot.candidates.push(name.to_owned()),
            Err(CopyError::Skip(reason)) => snapshot.warnings.push(skip_warning(name, reason)),
            Err(CopyError::Failed(error)) => {
                return Err(ApiError::Io(format!(
                    "Cannot inspect copy source {name:?}: {error}"
                )));
            }
        }
    }
    snapshot.source = Some(root);
    Ok(snapshot)
}

pub(super) fn apply(snapshot: CopySnapshot, destination: &Path) -> (bool, Vec<String>) {
    let CopySnapshot {
        source,
        candidates,
        mut warnings,
    } = snapshot;
    if candidates.is_empty() {
        return (true, warnings);
    }
    let Some(source) = source else {
        warnings.push(format!(
            "Could not copy into {}: source root is unavailable",
            destination.display()
        ));
        return (false, warnings);
    };
    let destination_root = match open_root(destination) {
        Ok(root) => root,
        Err(error) => {
            for name in candidates {
                warnings.push(format!(
                    "Could not copy {name:?}: cannot open destination {}: {error}",
                    destination.display()
                ));
            }
            return (false, warnings);
        }
    };
    // The index includes skip-worktree entries even when sparse checkout omits the files.
    let tracked = git_output(
        &fd_path(&destination_root),
        &["ls-files", "--cached", "-z", "--"],
        "enumerate destination tracked paths",
    )
    .and_then(|output| {
        utf8_paths(&output).map(|names| names.map(str::to_owned).collect::<BTreeSet<_>>())
    });
    let tracked = match tracked {
        Ok(paths) => paths,
        Err(error) => {
            for name in candidates {
                warnings.push(format!("Could not copy {name:?}: {error}"));
            }
            return (false, warnings);
        }
    };
    let mut complete = true;
    for name in candidates {
        let result = if tracked_conflict(&tracked, &name) {
            Err(CopyError::Skip("conflicts with a tracked destination path"))
        } else {
            copy_candidate(&source, &destination_root, &name)
        };
        match result {
            Ok(()) => {}
            Err(CopyError::Skip(reason)) => warnings.push(skip_warning(&name, reason)),
            Err(CopyError::Failed(error)) => {
                complete = false;
                warnings.push(format!("Could not copy {name:?}: {error}"));
            }
        }
    }
    (complete, warnings)
}

fn compile_patterns(patterns: &BTreeMap<String, bool>) -> Result<(GlobSet, GlobSet), ApiError> {
    let mut include = GlobSetBuilder::new();
    let mut veto = GlobSetBuilder::new();
    for (pattern, enabled) in patterns {
        let builder = if *enabled { &mut include } else { &mut veto };
        // Match the project-relative root form as well as the any-depth convention.
        for candidate in [Some(pattern.as_str()), pattern.strip_prefix("**/")]
            .into_iter()
            .flatten()
            .filter(|candidate| !candidate.is_empty())
        {
            let glob = GlobBuilder::new(candidate)
                .literal_separator(true)
                .build()
                .map_err(|error| {
                    ApiError::InvalidArgument(format!(
                        "Invalid git.worktree_copy glob {pattern:?}: {error}"
                    ))
                })?;
            builder.add(glob);
        }
    }
    let build = |builder: GlobSetBuilder| {
        builder.build().map_err(|error| {
            ApiError::InvalidArgument(format!("Invalid git.worktree_copy globs: {error}"))
        })
    };
    Ok((build(include)?, build(veto)?))
}

fn matches_path_or_parent(globs: &GlobSet, path: &Path) -> bool {
    path.ancestors()
        .take_while(|path| !path.as_os_str().is_empty())
        .any(|path| globs.is_match(path))
}

fn utf8_paths(output: &[u8]) -> Result<impl Iterator<Item = &str>, String> {
    let text = std::str::from_utf8(output)
        .map_err(|_| "Worktree copy paths must be valid UTF-8".to_owned())?;
    Ok(text.split('\0').filter(|name| !name.is_empty()))
}

fn relative_path(name: &str) -> Result<&Path, &'static str> {
    let path = Path::new(name);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("expected a relative path without '.' or '..' components");
    }
    Ok(path)
}

fn has_git_component(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == ".git")
}

fn tracked_conflict(tracked: &BTreeSet<String>, name: &str) -> bool {
    if Path::new(name)
        .ancestors()
        .filter_map(Path::to_str)
        .any(|ancestor| tracked.contains(ancestor))
    {
        return true;
    }
    let prefix = format!("{}/", name.trim_end_matches('/'));
    tracked
        .range::<str, _>((
            std::ops::Bound::Included(prefix.as_str()),
            std::ops::Bound::Unbounded,
        ))
        .next()
        .is_some_and(|path| path.starts_with(&prefix))
}

#[derive(Debug)]
enum CopyError {
    Skip(&'static str),
    Failed(io::Error),
}

impl From<io::Error> for CopyError {
    fn from(error: io::Error) -> Self {
        Self::Failed(error)
    }
}

fn skip_warning(name: &str, reason: &str) -> String {
    format!("Skipped {name:?}: {reason}")
}

fn fd_path(file: &File) -> PathBuf {
    // Command resolves cwd before exec closes O_CLOEXEC descriptors.
    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

fn c_name(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"))
}

fn open_at(parent: RawFd, name: &CStr, flags: libc::c_int, mode: libc::mode_t) -> io::Result<File> {
    // SAFETY: name is NUL-terminated; the returned descriptor is uniquely owned.
    let fd = unsafe { libc::openat(parent, name.as_ptr(), flags | libc::O_CLOEXEC, mode) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

fn open_root(path: &Path) -> io::Result<File> {
    let start = if path.is_absolute() { c"/" } else { c"." };
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW;
    let mut root = open_at(libc::AT_FDCWD, start, flags, 0)?;
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => root = open_at(root.as_raw_fd(), &c_name(name)?, flags, 0)?,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unsafe root path",
                ))
            }
        }
    }
    Ok(root)
}

fn stat_at(parent: &File, name: &CStr) -> io::Result<libc::stat> {
    let mut stat = MaybeUninit::uninit();
    // SAFETY: the name and output storage are valid for this call; no links are followed.
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } < 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { stat.assume_init() })
    }
}

fn optional_stat(parent: &File, name: &CStr) -> io::Result<Option<libc::stat>> {
    match stat_at(parent, name) {
        Ok(stat) => Ok(Some(stat)),
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => Ok(None),
        Err(error) => Err(error),
    }
}

fn is_repository(directory: &File) -> io::Result<bool> {
    // A .git file also covers submodules and linked worktrees; never follow it.
    if optional_stat(directory, c".git")?.is_some() {
        return Ok(true);
    }
    // Only probe Git when the directory has the shape of a bare repository.
    let head = optional_stat(directory, c"HEAD")?;
    if !head
        .is_some_and(|stat| matches!(stat.st_mode & libc::S_IFMT, libc::S_IFREG | libc::S_IFLNK))
    {
        return Ok(false);
    }
    let objects = optional_stat(directory, c"objects")?;
    if !objects
        .is_some_and(|stat| matches!(stat.st_mode & libc::S_IFMT, libc::S_IFDIR | libc::S_IFLNK))
    {
        return Ok(false);
    }
    let output = git_command(&fd_path(directory))
        .args(["--no-optional-locks", "rev-parse", "--is-bare-repository"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Cannot inspect nested repository: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout == b"true\n")
}

fn parent_directory(
    root: &File,
    path: &Path,
    create: bool,
    reject_repositories: bool,
) -> Result<(File, CString), CopyError> {
    let mut parent = root.try_clone()?;
    let mut components = path.components().peekable();
    while let Some(Component::Normal(component)) = components.next() {
        let name = c_name(component)?;
        if components.peek().is_none() {
            return Ok((parent, name));
        }
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW;
        let mut opened = open_at(parent.as_raw_fd(), &name, flags, 0);
        if create
            && opened
                .as_ref()
                .is_err_and(|error| error.raw_os_error() == Some(libc::ENOENT))
        {
            // SAFETY: this is a single component relative to the anchored parent.
            if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o755) } < 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::EEXIST) {
                    return Err(error.into());
                }
            }
            opened = open_at(parent.as_raw_fd(), &name, flags, 0);
        }
        parent = match opened {
            Ok(directory) => directory,
            Err(error) if matches!(error.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR)) => {
                return Err(CopyError::Skip("symlink or non-directory parent refused"));
            }
            Err(error) => return Err(error.into()),
        };
        if reject_repositories && is_repository(&parent)? {
            return Err(CopyError::Skip("nested repository is never copied"));
        }
    }
    Err(CopyError::Skip("unsafe relative path"))
}

fn inspect_source(root: &File, path: &Path) -> Result<(), CopyError> {
    let (parent, name) = parent_directory(root, path, false, true)?;
    let stat = stat_at(&parent, &name)?;
    if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
        let directory = open_at(
            parent.as_raw_fd(),
            &name,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            0,
        )?;
        return Err(CopyError::Skip(if is_repository(&directory)? {
            "nested repository is never copied"
        } else {
            "only regular files are copied"
        }));
    }
    Ok(())
}

fn copy_candidate(
    source_root: &File,
    destination_root: &File,
    name: &str,
) -> Result<(), CopyError> {
    let path = relative_path(name).map_err(CopyError::Skip)?;
    if has_git_component(path) {
        return Err(CopyError::Skip("Git metadata is never copied"));
    }
    let (source_parent, source_name) = parent_directory(source_root, path, false, true)?;
    let stat = stat_at(&source_parent, &source_name)?;
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(CopyError::Skip(
            "symlinks and special files are never copied",
        ));
    }
    let mut source = match open_at(
        source_parent.as_raw_fd(),
        &source_name,
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        0,
    ) {
        Ok(source) => source,
        Err(error)
            if matches!(
                error.raw_os_error(),
                Some(libc::ELOOP | libc::ENXIO | libc::ENODEV)
            ) =>
        {
            return Err(CopyError::Skip("symlink or special source refused"));
        }
        Err(error) => return Err(error.into()),
    };
    let metadata = source.metadata()?;
    if !metadata.is_file() {
        return Err(CopyError::Skip("only regular files are copied"));
    }
    let (destination_parent, destination_name) =
        parent_directory(destination_root, path, true, true)?;
    let mut destination = match open_at(
        destination_parent.as_raw_fd(),
        &destination_name,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
        0o600,
    ) {
        Ok(destination) => destination,
        Err(error) if error.raw_os_error() == Some(libc::EEXIST) => {
            return Err(CopyError::Skip("destination already exists"));
        }
        Err(error) => return Err(error.into()),
    };
    let copied = copy_contents(&mut source, &mut destination).and_then(|()| {
        destination.set_permissions(Permissions::from_mode(metadata.mode() & 0o777))
    });
    if let Err(error) = copied {
        let cleanup = remove_partial(&destination_parent, &destination_name, &destination);
        return Err(CopyError::Failed(match cleanup {
            Ok(()) => error,
            Err(cleanup) => {
                io::Error::other(format!("{error}; partial-file cleanup failed: {cleanup}"))
            }
        }));
    }
    Ok(())
}

fn copy_contents(source: &mut File, destination: &mut File) -> io::Result<()> {
    // SAFETY: FICLONE takes these live descriptors and does not take ownership.
    if unsafe { libc::ioctl(destination.as_raw_fd(), 0x40049409 as _, source.as_raw_fd()) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if !matches!(
        error.raw_os_error(),
        Some(libc::EOPNOTSUPP | libc::ENOTTY | libc::ENOSYS | libc::EINVAL | libc::EXDEV)
    ) {
        return Err(error);
    }
    destination.set_len(0)?;
    io::copy(source, destination).map(|_| ())
}

fn remove_partial(parent: &File, name: &CStr, created: &File) -> io::Result<()> {
    let original = created.metadata()?;
    let current = match stat_at(parent, name) {
        Ok(stat) => stat,
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => return Ok(()),
        Err(error) => return Err(error),
    };
    // Do not unlink a replacement installed while the copy was in flight.
    if current.st_dev != original.dev() || current.st_ino != original.ino() {
        return Ok(());
    }
    // SAFETY: unlink just this entry under the held parent, never a re-resolved path.
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } < 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ENOENT) {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;
    use std::os::unix::net::UnixListener;
    use sworm_protocol::git::GitWorktree;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(parent: &Path, tag: &str) -> io::Result<Self> {
            let path = parent.join(format!(
                "sworm-worktree-copy-{tag}-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            fs::create_dir(&path)?;
            Ok(Self(path))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct TestRepo {
        directory: TestDirectory,
        source: PathBuf,
    }

    impl TestRepo {
        fn new(tag: &str) -> Self {
            let directory = TestDirectory::new(&std::env::temp_dir(), tag).unwrap();
            let source = directory.0.join("source");
            fs::create_dir(&source).unwrap();
            git(&source, &["init", "-q", "-b", "main"]);
            git(&source, &["config", "user.email", "copy@example.invalid"]);
            git(&source, &["config", "user.name", "Copy test"]);
            fs::write(source.join("seed"), "tracked\n").unwrap();
            git(&source, &["add", "seed"]);
            git(&source, &["commit", "-q", "-m", "initial"]);
            Self { directory, source }
        }

        fn inventory(&self) -> GitWorktreeList {
            GitWorktreeList {
                common_dir: self.source.join(".git").to_str().unwrap().to_owned(),
                main_path: Some(self.source.to_str().unwrap().to_owned()),
                worktrees: vec![worktree(&self.source)],
            }
        }

        fn destination(&self) -> PathBuf {
            let path = self.directory.0.join("destination");
            self.add_destination(&path);
            path
        }

        fn add_destination(&self, path: &Path) {
            git(
                &self.source,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "--detach",
                    path.to_str().unwrap(),
                    "HEAD",
                ],
            );
        }

        fn snapshot(&self, patterns: &[(&str, bool)]) -> CopySnapshot {
            snapshot(&self.source, &self.inventory(), &pattern_map(patterns)).unwrap()
        }
    }

    fn worktree(path: &Path) -> GitWorktree {
        GitWorktree {
            path: path.to_str().unwrap().to_owned(),
            head: None,
            branch: None,
            is_main: false,
            is_current: false,
            bare: false,
            locked: None,
            prunable: None,
        }
    }

    fn pattern_map(patterns: &[(&str, bool)]) -> BTreeMap<String, bool> {
        patterns
            .iter()
            .map(|(pattern, enabled)| ((*pattern).to_owned(), *enabled))
            .collect()
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = git_command(repo).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn write(root: &Path, path: &str, contents: &str) {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, contents).unwrap();
    }

    #[test]
    fn empty_patterns_do_not_touch_source_or_destination() {
        let inventory = GitWorktreeList {
            common_dir: String::new(),
            main_path: None,
            worktrees: Vec::new(),
        };
        let snapshot = snapshot(
            Path::new("/nonexistent/sworm-copy-source"),
            &inventory,
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(
            apply(snapshot, Path::new("/nonexistent/sworm-copy-destination")),
            (true, Vec::new())
        );
    }

    #[test]
    fn invalid_include_and_veto_globs_fail_before_enumeration() {
        let inventory = GitWorktreeList {
            common_dir: String::new(),
            main_path: None,
            worktrees: Vec::new(),
        };
        for enabled in [true, false] {
            let result = snapshot(
                Path::new("/nonexistent/sworm-copy-source"),
                &inventory,
                &pattern_map(&[("[", enabled)]),
            );
            assert!(matches!(result, Err(ApiError::InvalidArgument(_))));
        }
    }

    #[test]
    fn selection_includes_ignored_and_parent_matches_with_false_veto() {
        let repo = TestRepo::new("selection");
        write(&repo.source, ".gitignore", "ignored/\n.env\n");
        git(&repo.source, &["add", ".gitignore"]);
        git(&repo.source, &["commit", "-q", "-m", "ignore"]);
        for path in [
            ".env",
            "deep/.env",
            "ignored/settings",
            "ignored/private/secret",
            "cache/shared/config",
            "cache/shared/private/secret",
            "cache/too/deep/config",
            "unselected",
        ] {
            write(&repo.source, path, path);
        }
        let snapshot = repo.snapshot(&[
            ("**/.env", true),
            ("ignored", true),
            ("cache/*", true),
            ("**/private", false),
            ("cache/too", false),
        ]);
        let destination = repo.destination();
        assert_eq!(apply(snapshot, &destination), (true, Vec::new()));
        for path in [
            ".env",
            "deep/.env",
            "ignored/settings",
            "cache/shared/config",
        ] {
            assert_eq!(fs::read_to_string(destination.join(path)).unwrap(), path);
        }
        for path in [
            "ignored/private/secret",
            "cache/shared/private/secret",
            "cache/too/deep/config",
            "unselected",
        ] {
            assert!(!destination.join(path).exists(), "unexpected copy: {path}");
        }
        assert_eq!(
            fs::read_to_string(destination.join("seed")).unwrap(),
            "tracked\n"
        );
    }

    #[test]
    fn literal_separator_does_not_make_single_star_recursive() {
        let (include, _) = compile_patterns(&pattern_map(&[("cache/*.env", true)])).unwrap();
        assert!(matches_path_or_parent(&include, Path::new("cache/a.env")));
        assert!(!matches_path_or_parent(
            &include,
            Path::new("cache/deep/a.env")
        ));
    }

    #[test]
    fn non_utf8_candidates_are_rejected_without_lossy_conversion() {
        let repo = TestRepo::new("non-utf8");
        let filename = std::ffi::OsString::from_vec(vec![b'b', 0xff]);
        fs::write(repo.source.join(filename), "secret").unwrap();
        assert!(matches!(
            snapshot(
                &repo.source,
                &repo.inventory(),
                &pattern_map(&[("**", true)])
            ),
            Err(ApiError::InvalidArgument(_))
        ));
    }

    #[test]
    fn tracked_sparse_paths_block_exact_ancestor_and_descendant_copies() {
        let repo = TestRepo::new("sparse");
        for path in ["exact", "ancestor", "descendant/child"] {
            write(&repo.source, path, "tracked destination");
        }
        git(
            &repo.source,
            &["add", "exact", "ancestor", "descendant/child"],
        );
        git(&repo.source, &["commit", "-q", "-m", "destination paths"]);
        let destination = repo.destination();
        git(&destination, &["sparse-checkout", "init", "--no-cone"]);
        git(
            &destination,
            &["sparse-checkout", "set", "--no-cone", "/seed"],
        );
        for path in ["exact", "ancestor", "descendant"] {
            assert!(!destination.join(path).exists());
        }
        git(
            &repo.source,
            &["rm", "-q", "exact", "ancestor", "descendant/child"],
        );
        write(&repo.source, "exact", "do not copy");
        write(&repo.source, "ancestor/child", "do not copy");
        write(&repo.source, "descendant", "do not copy");
        let snapshot = repo.snapshot(&[("**", true)]);
        assert_eq!(snapshot.candidates.len(), 3);
        let (complete, warnings) = apply(snapshot, &destination);
        assert!(complete);
        assert_eq!(warnings.len(), 3);
        assert!(warnings
            .iter()
            .all(|warning| warning.contains("tracked destination")));
        for path in ["exact", "ancestor", "descendant"] {
            assert!(!destination.join(path).exists());
        }
    }

    #[test]
    fn nested_registered_bare_and_submodule_repositories_are_excluded() {
        let repo = TestRepo::new("nested");
        let nested = repo.source.join("nested");
        fs::create_dir(&nested).unwrap();
        git(&nested, &["init", "-q"]);
        write(&nested, "secret.env", "nested secret");
        let bare = repo.source.join("bare");
        fs::create_dir(&bare).unwrap();
        git(&bare, &["init", "-q", "--bare"]);
        write(&bare, "secret.env", "bare secret");
        // Bare repositories can legitimately store objects through a symlink.
        let objects = repo.directory.0.join("bare-objects");
        fs::rename(bare.join("objects"), &objects).unwrap();
        symlink(&objects, bare.join("objects")).unwrap();
        let registered = repo.source.join("registered");
        repo.add_destination(&registered);
        write(&registered, "secret.env", "registered secret");
        let module = TestRepo::new("module");
        git(
            &repo.source,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                module.source.to_str().unwrap(),
                "module",
            ],
        );
        write(&repo.source, "module/secret.env", "submodule secret");
        write(&repo.source, "safe.env", "safe");
        let mut inventory = repo.inventory();
        inventory.worktrees.push(worktree(&registered));
        let snapshot = snapshot(&repo.source, &inventory, &pattern_map(&[("**", true)])).unwrap();
        assert_eq!(snapshot.candidates, ["safe.env"]);
        let destination = repo.destination();
        let (complete, _) = apply(snapshot, &destination);
        assert!(complete);
        assert_eq!(
            fs::read_to_string(destination.join("safe.env")).unwrap(),
            "safe"
        );
        for path in ["nested", "bare", "registered", "module"] {
            assert!(
                !destination.join(path).exists(),
                "copied repository: {path}"
            );
        }
    }

    #[test]
    fn source_and_destination_symlink_parents_are_refused() {
        for replace_source in [true, false] {
            let repo = TestRepo::new("parent-symlink");
            write(&repo.source, "parent/file.env", "inside");
            let snapshot = repo.snapshot(&[("**/*.env", true)]);
            let destination = repo.destination();
            let outside = repo.directory.0.join("outside");
            fs::create_dir(&outside).unwrap();
            write(&outside, "file.env", "outside");
            if replace_source {
                fs::rename(repo.source.join("parent"), repo.source.join("old-parent")).unwrap();
                symlink(&outside, repo.source.join("parent")).unwrap();
            } else {
                symlink(&outside, destination.join("parent")).unwrap();
            }
            let (complete, warnings) = apply(snapshot, &destination);
            assert!(complete);
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].contains("parent/file.env"));
            assert!(warnings[0].contains("parent refused"));
            assert_eq!(
                fs::read_to_string(outside.join("file.env")).unwrap(),
                "outside"
            );
            if replace_source {
                assert!(!destination.join("parent").exists());
            }
        }
    }

    #[test]
    fn roots_and_their_ancestors_never_follow_symlinks() {
        let repo = TestRepo::new("root-symlink");
        let alias = repo.directory.0.join("alias");
        symlink(&repo.source, &alias).unwrap();
        assert!(open_root(&alias).is_err());
        assert!(open_root(&alias.join(".git")).is_err());
        write(&repo.source, "file.env", "source");
        let snapshot = repo.snapshot(&[("**/*.env", true)]);
        let destination = repo.destination();
        let destination_alias = repo.directory.0.join("destination-alias");
        symlink(&destination, &destination_alias).unwrap();
        let (complete, warnings) = apply(snapshot, &destination_alias);
        assert!(!complete);
        assert!(warnings[0].contains("file.env"));
        assert!(!destination.join("file.env").exists());
    }

    #[test]
    fn source_symlinks_fifos_and_sockets_are_successful_skips() {
        let repo = TestRepo::new("special-files");
        write(&repo.source, "regular.env", "source");
        symlink("regular.env", repo.source.join("link.env")).unwrap();
        let fifo = c_name(repo.source.join("fifo.env").as_os_str()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        // Keep sockaddr_un bounded even when TMPDIR has a long pathname.
        let socket_root = open_root(&repo.source).unwrap();
        let _listener = UnixListener::bind(fd_path(&socket_root).join("socket.env")).unwrap();
        let mut snapshot = repo.snapshot(&[("**/*.env", true)]);
        // Git versions may omit special entries; exercise the real descriptors regardless.
        for name in ["fifo.env", "socket.env"] {
            if !snapshot
                .candidates
                .iter()
                .any(|candidate| candidate == name)
            {
                snapshot.candidates.push(name.to_owned());
            }
        }
        let destination = repo.destination();
        let (complete, warnings) = apply(snapshot, &destination);
        assert!(complete);
        assert_eq!(warnings.len(), 3);
        for name in ["link.env", "fifo.env", "socket.env"] {
            assert!(!destination.join(name).exists());
            assert!(warnings.iter().any(|warning| warning.contains(name)));
        }
        assert_eq!(
            fs::read_to_string(destination.join("regular.env")).unwrap(),
            "source"
        );
    }

    #[test]
    fn existing_destination_files_directories_and_symlinks_are_never_overwritten() {
        let repo = TestRepo::new("existing");
        for name in ["file.env", "directory.env", "link.env"] {
            write(&repo.source, name, "source");
        }
        let snapshot = repo.snapshot(&[("**/*.env", true)]);
        let destination = repo.destination();
        write(&destination, "file.env", "existing");
        fs::create_dir(destination.join("directory.env")).unwrap();
        symlink("file.env", destination.join("link.env")).unwrap();
        let (complete, warnings) = apply(snapshot, &destination);
        assert!(complete);
        assert_eq!(warnings.len(), 3);
        assert_eq!(
            fs::read_to_string(destination.join("file.env")).unwrap(),
            "existing"
        );
        assert!(destination.join("directory.env").is_dir());
        assert_eq!(
            fs::read_link(destination.join("link.env")).unwrap(),
            Path::new("file.env")
        );
    }

    #[test]
    fn copy_preserves_ordinary_executable_mode_and_independent_contents() {
        let repo = TestRepo::new("mode-independence");
        write(&repo.source, "script.sh", "#!/bin/sh\necho source\n");
        fs::set_permissions(
            repo.source.join("script.sh"),
            Permissions::from_mode(0o6751),
        )
        .unwrap();
        let snapshot = repo.snapshot(&[("*.sh", true)]);
        let destination = repo.destination();
        assert_eq!(apply(snapshot, &destination), (true, Vec::new()));
        let source_metadata = fs::metadata(repo.source.join("script.sh")).unwrap();
        let copied_metadata = fs::metadata(destination.join("script.sh")).unwrap();
        assert_eq!(copied_metadata.mode() & 0o7777, 0o751);
        assert_ne!(
            (source_metadata.dev(), source_metadata.ino()),
            (copied_metadata.dev(), copied_metadata.ino())
        );
        write(&destination, "script.sh", "destination changed");
        assert_eq!(
            fs::read_to_string(repo.source.join("script.sh")).unwrap(),
            "#!/bin/sh\necho source\n"
        );
        write(&repo.source, "script.sh", "source changed");
        assert_eq!(
            fs::read_to_string(destination.join("script.sh")).unwrap(),
            "destination changed"
        );
    }

    #[test]
    fn snapshot_records_names_not_bytes_and_keeps_the_original_root() {
        let repo = TestRepo::new("snapshot");
        write(&repo.source, "file.env", "before");
        let snapshot = repo.snapshot(&[("**/*.env", true)]);
        // Use an independent repository: moving the source invalidates linked worktree metadata.
        let destination = TestRepo::new("snapshot-destination");
        let original = repo.directory.0.join("original-source");
        fs::rename(&repo.source, &original).unwrap();
        fs::create_dir(&repo.source).unwrap();
        write(&repo.source, "file.env", "replacement root");
        write(&original, "file.env", "after snapshot");
        assert_eq!(apply(snapshot, &destination.source), (true, Vec::new()));
        assert_eq!(
            fs::read_to_string(destination.source.join("file.env")).unwrap(),
            "after snapshot"
        );
    }

    #[test]
    fn failed_copy_preserves_checkout_and_reports_candidate_path() {
        let repo = TestRepo::new("failure");
        write(&repo.source, "missing.env", "source");
        let snapshot = repo.snapshot(&[("**/*.env", true)]);
        let destination = repo.destination();
        fs::remove_file(repo.source.join("missing.env")).unwrap();
        let (complete, warnings) = apply(snapshot, &destination);
        assert!(!complete);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("missing.env"));
        assert_eq!(
            fs::read_to_string(destination.join("seed")).unwrap(),
            "tracked\n"
        );
        git(&destination, &["rev-parse", "--verify", "HEAD"]);
        assert!(!destination.join("missing.env").exists());
    }

    #[test]
    fn partial_cleanup_is_anchored_and_retains_replacement_entries() {
        let directory = TestDirectory::new(&std::env::temp_dir(), "cleanup").unwrap();
        let parent = directory.0.join("parent");
        fs::create_dir(&parent).unwrap();
        let anchored = open_root(&parent).unwrap();
        let name = c_name(OsStr::new("partial")).unwrap();
        let created = open_at(
            anchored.as_raw_fd(),
            &name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
            0o600,
        )
        .unwrap();
        fs::remove_file(parent.join("partial")).unwrap();
        write(&parent, "partial", "replacement");
        remove_partial(&anchored, &name, &created).unwrap();
        assert_eq!(
            fs::read_to_string(parent.join("partial")).unwrap(),
            "replacement"
        );
        let own_name = c_name(OsStr::new("own-partial")).unwrap();
        let own = open_at(
            anchored.as_raw_fd(),
            &own_name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
            0o600,
        )
        .unwrap();
        let moved = directory.0.join("moved-parent");
        fs::rename(&parent, &moved).unwrap();
        let outside = directory.0.join("outside");
        fs::create_dir(&outside).unwrap();
        write(&outside, "own-partial", "outside");
        symlink(&outside, &parent).unwrap();
        remove_partial(&anchored, &own_name, &own).unwrap();
        assert!(!moved.join("own-partial").exists());
        assert_eq!(
            fs::read_to_string(outside.join("own-partial")).unwrap(),
            "outside"
        );
    }

    #[test]
    fn unsafe_candidates_and_git_metadata_are_refused() {
        let repo = TestRepo::new("unsafe-paths");
        let destination = repo.destination();
        let source = open_root(&repo.source).unwrap();
        let destination = open_root(&destination).unwrap();
        for name in [
            "/tmp/outside",
            "../outside",
            "parent/../../outside",
            ".git/config",
            "parent/.git/config",
        ] {
            assert!(matches!(
                copy_candidate(&source, &destination, name),
                Err(CopyError::Skip(_))
            ));
        }
    }

    #[test]
    fn cross_filesystem_copy_falls_back_when_a_second_filesystem_is_available() {
        let repo = TestRepo::new("cross-filesystem");
        let source_device = fs::metadata(&repo.source).unwrap().dev();
        let second = [Path::new("/dev/shm"), Path::new("/tmp")]
            .into_iter()
            .filter(|path| {
                fs::metadata(path)
                    .is_ok_and(|metadata| metadata.is_dir() && metadata.dev() != source_device)
            })
            .find_map(|path| TestDirectory::new(path, "cross-filesystem-destination").ok());
        let Some(second) = second else {
            return;
        };
        write(&repo.source, "file.env", "cross filesystem contents");
        let snapshot = repo.snapshot(&[("**/*.env", true)]);
        let destination = second.0.join("destination");
        repo.add_destination(&destination);
        assert_eq!(apply(snapshot, &destination), (true, Vec::new()));
        assert_eq!(
            fs::read_to_string(destination.join("file.env")).unwrap(),
            "cross filesystem contents"
        );
        write(&destination, "file.env", "independent");
        assert_eq!(
            fs::read_to_string(repo.source.join("file.env")).unwrap(),
            "cross filesystem contents"
        );
    }
}
