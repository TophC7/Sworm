//! Decides what the file explorer shows.
//!
//! Two independent mechanisms, mirroring VS Code's `files.exclude` and
//! `explorer.excludeGitIgnore`:
//! - **exclude globs** hide entries outright (`.git`, OS droppings, whatever the
//!   user configures);
//! - **gitignore matching** marks entries as ignored so they can be dimmed, or
//!   hidden when the user opts into `explorer.exclude_gitignore`.
//!
//! Gitignore state is evaluated lazily per directory, so opening a folder never
//! pays for the whole tree.

use crate::errors::ApiError;
use crate::models::settings::ExplorerSettings;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::Match;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// Identity of a `.gitignore` file, used to detect edits without inotify.
type IgnoreStamp = Option<(SystemTime, u64)>;

pub struct ExplorerFilter {
    root: PathBuf,
    exclude: GlobSet,
    pub exclude_gitignore: bool,
    pub compact_folders: bool,
    /// Enclosing git repository, which may sit above the opened folder — the
    /// search walk resolves it from ancestors too, so both must agree.
    repo_root: Option<PathBuf>,
    /// `.git/info/exclude`, which git applies repo-wide.
    repo_exclude: Gitignore,
    /// `core.excludesFile`, which git applies to every repo.
    global_exclude: Gitignore,
    /// Per-directory `.gitignore` matchers, revalidated against the ignore
    /// file's mtime + size so an edit takes effect without any invalidation
    /// plumbing between the watcher and this cache.
    per_dir: Mutex<HashMap<PathBuf, (IgnoreStamp, Arc<Gitignore>)>>,
}

impl ExplorerFilter {
    pub fn build(root: &Path, settings: &ExplorerSettings) -> Result<Self, ApiError> {
        let mut globs = GlobSetBuilder::new();
        for (pattern, enabled) in &settings.exclude {
            if !enabled {
                continue;
            }
            // A leading `**/` is how VS Code spells "at any depth", but the
            // paths we match are project-relative, so the root-level form has
            // to be registered explicitly too.
            let candidates = [Some(pattern.as_str()), pattern.strip_prefix("**/")];
            for candidate in candidates.into_iter().flatten() {
                if candidate.is_empty() {
                    continue;
                }
                match GlobBuilder::new(candidate).literal_separator(true).build() {
                    Ok(glob) => {
                        globs.add(glob);
                    }
                    Err(error) => tracing::warn!(
                        glob = candidate,
                        %error,
                        "ignoring invalid explorer.exclude glob"
                    ),
                }
            }
        }
        let exclude = globs
            .build()
            .map_err(|error| ApiError::Io(format!("Invalid explorer.exclude globs: {error}")))?;

        let repo_root = root
            .ancestors()
            .find(|dir| dir.join(".git").exists())
            .map(Path::to_path_buf);
        let (repo_exclude, global_exclude) = match &repo_root {
            Some(repo) => {
                let mut builder = GitignoreBuilder::new(repo);
                builder.add(repo.join(".git").join("info").join("exclude"));
                let repo = builder.build().unwrap_or_else(|_| Gitignore::empty());
                (repo, Gitignore::global().0)
            }
            None => (Gitignore::empty(), Gitignore::empty()),
        };

        Ok(Self {
            root: root.to_path_buf(),
            exclude,
            exclude_gitignore: settings.exclude_gitignore,
            compact_folders: settings.compact_folders,
            repo_root,
            repo_exclude,
            global_exclude,
            per_dir: Mutex::new(HashMap::new()),
        })
    }

    /// Matched by an `explorer.exclude` glob. `rel_path` is project-relative
    /// with forward slashes.
    pub fn is_excluded(&self, rel_path: &str) -> bool {
        self.exclude.is_match(rel_path)
    }

    /// The `.gitignore` matchers governing the entries of `rel_dir`,
    /// deepest-first — that ordering is how git resolves a nested `!negation`.
    /// Resolved once per listing because every entry shares the same chain.
    pub fn ignore_chain(&self, rel_dir: &str) -> IgnoreChain<'_> {
        let mut matchers = Vec::new();
        if self.repo_root.is_some() {
            let mut dir = Some(self.root.join(rel_dir));
            while let Some(abs_dir) = dir {
                matchers.push(self.matcher_for(&abs_dir));
                if Some(abs_dir.as_path()) == self.repo_root.as_deref() {
                    break;
                }
                dir = abs_dir.parent().map(Path::to_path_buf);
            }
        }
        IgnoreChain {
            filter: self,
            matchers,
        }
    }

    /// Matched by git's ignore rules. Prefer `ignore_chain` when testing more
    /// than one entry of the same directory.
    pub fn is_ignored(&self, rel_path: &str, is_dir: bool) -> bool {
        let parent = rel_path.rsplit_once('/').map_or("", |(dir, _)| dir);
        self.ignore_chain(parent).is_ignored(rel_path, is_dir)
    }

    fn matcher_for(&self, abs_dir: &Path) -> Arc<Gitignore> {
        let ignore_file = abs_dir.join(".gitignore");
        let stamp = std::fs::metadata(&ignore_file)
            .and_then(|metadata| Ok((metadata.modified()?, metadata.len())))
            .ok();

        let mut cache = self.per_dir.lock();
        if let Some((cached_stamp, matcher)) = cache.get(abs_dir) {
            if *cached_stamp == stamp {
                return Arc::clone(matcher);
            }
        }

        let mut builder = GitignoreBuilder::new(abs_dir);
        if stamp.is_some() {
            builder.add(&ignore_file);
        }
        let matcher = Arc::new(builder.build().unwrap_or_else(|_| Gitignore::empty()));
        cache.insert(abs_dir.to_path_buf(), (stamp, Arc::clone(&matcher)));
        matcher
    }
}

/// The ignore rules that apply to one directory's entries, resolved once.
pub struct IgnoreChain<'a> {
    filter: &'a ExplorerFilter,
    matchers: Vec<Arc<Gitignore>>,
}

impl IgnoreChain<'_> {
    /// `rel_path` is project-relative with forward slashes and must live in the
    /// directory this chain was built for.
    pub fn is_ignored(&self, rel_path: &str, is_dir: bool) -> bool {
        if self.matchers.is_empty() || rel_path.is_empty() {
            return false;
        }

        let abs = self.filter.root.join(rel_path);
        for matcher in &self.matchers {
            match matcher.matched(&abs, is_dir) {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            }
        }

        matches!(
            self.filter.repo_exclude.matched(&abs, is_dir),
            Match::Ignore(_)
        ) || matches!(
            self.filter.global_exclude.matched(&abs, is_dir),
            Match::Ignore(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn unique_test_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "sworm-filter-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    fn settings(exclude: &[(&str, bool)]) -> ExplorerSettings {
        ExplorerSettings {
            exclude: exclude
                .iter()
                .map(|(glob, enabled)| ((*glob).to_string(), *enabled))
                .collect::<BTreeMap<_, _>>(),
            exclude_gitignore: false,
            compact_folders: true,
        }
    }

    #[test]
    fn exclude_glob_matches_root_and_nested_paths() {
        let dir = unique_test_dir("globs");
        let filter =
            ExplorerFilter::build(&dir, &settings(&[("**/.git", true), ("**/vendor", true)]))
                .expect("build filter");

        assert!(filter.is_excluded(".git"));
        assert!(filter.is_excluded("a/b/.git"));
        assert!(filter.is_excluded("vendor"));
        assert!(!filter.is_excluded("vendored.rs"));
        assert!(!filter.is_excluded("src/main.rs"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn disabled_exclude_entry_is_not_applied() {
        let dir = unique_test_dir("disabled");
        let filter =
            ExplorerFilter::build(&dir, &settings(&[("**/.git", false)])).expect("build filter");

        assert!(!filter.is_excluded(".git"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nested_gitignore_negation_wins_over_parent() {
        let dir = unique_test_dir("nested-ignore");
        std::fs::create_dir_all(dir.join(".git")).expect("fake git dir");
        std::fs::create_dir_all(dir.join("sub")).expect("sub dir");
        std::fs::write(dir.join(".gitignore"), "build/\n*.log\n").expect("root ignore");
        std::fs::write(dir.join("sub/.gitignore"), "!keep.log\n").expect("sub ignore");

        let filter = ExplorerFilter::build(&dir, &settings(&[])).expect("build filter");

        assert!(!filter.is_ignored("sub/keep.log", false));
        assert!(filter.is_ignored("sub/other.log", false));
        assert!(filter.is_ignored("other.log", false));
        assert!(filter.is_ignored("build", true));
        assert!(!filter.is_ignored("src/main.rs", false));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_git_repo_means_nothing_ignored() {
        let dir = unique_test_dir("no-git");
        std::fs::write(dir.join(".gitignore"), "*.log\n").expect("ignore file");

        let filter = ExplorerFilter::build(&dir, &settings(&[])).expect("build filter");

        assert!(!filter.is_ignored("app.log", false));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn edited_gitignore_takes_effect_without_rebuilding_filter() {
        let dir = unique_test_dir("edited-ignore");
        std::fs::create_dir_all(dir.join(".git")).expect("fake git dir");
        std::fs::write(dir.join(".gitignore"), "*.log\n").expect("ignore file");

        let filter = ExplorerFilter::build(&dir, &settings(&[])).expect("build filter");
        assert!(filter.is_ignored("app.log", false));

        std::fs::write(dir.join(".gitignore"), "*.tmp\n").expect("rewrite ignore file");
        assert!(!filter.is_ignored("app.log", false));
        assert!(filter.is_ignored("app.tmp", false));

        std::fs::remove_dir_all(&dir).ok();
    }
}
