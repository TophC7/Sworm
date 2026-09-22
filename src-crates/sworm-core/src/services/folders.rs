use crate::errors::ApiError;
use std::path::{Component, Path, PathBuf};
use sworm_protocol::folder::{PathRoot, PathRootKind};

/// Canonicalize a user-supplied folder path and require it to be a directory.
pub fn resolve_folder(path: &str) -> Result<PathBuf, ApiError> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| ApiError::NotFound(format!("Folder not found: {path}")))?;
    if !canonical.is_dir() {
        return Err(ApiError::InvalidArgument(format!(
            "Not a directory: {path}"
        )));
    }
    Ok(canonical)
}

/// Normalize an absolute path lexically without resolving symlinks.
pub fn normalize_absolute_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    normalized.pop();
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }

    normalized
}

/// Basename of a folder, falling back to the full path for roots like `/`.
pub fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// The user's home directory from `$HOME`; provider state lives beneath it.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Where the folder switcher's path bar starts for the canonical `path`,
/// following Nautilus: a user-visible mount, else Home, else the root
/// filesystem named by its label.
pub fn find_path_root(path: &Path) -> PathRoot {
    let mounts = read_mounts();
    let home = home_dir().and_then(|home| std::fs::canonicalize(home).ok());
    let lossy = |path: &Path| path.to_string_lossy().into_owned();
    match anchor_for(path, &mounts, home.as_deref()) {
        Anchor::Mount(mount_point, source) => PathRoot {
            kind: PathRootKind::Volume,
            label: device_label(source).unwrap_or_else(|| folder_name(mount_point)),
            path: lossy(mount_point),
        },
        Anchor::Home(home) => PathRoot {
            kind: PathRootKind::Home,
            label: "Home".to_owned(),
            path: lossy(home),
        },
        Anchor::Root(source) => PathRoot {
            kind: PathRootKind::Volume,
            label: source
                .and_then(device_label)
                .or_else(os_name)
                .unwrap_or_else(|| "Computer".to_owned()),
            path: "/".to_owned(),
        },
    }
}

#[derive(Debug, PartialEq)]
enum Anchor<'a> {
    /// `(mount point, mount source)` of a user-visible mount.
    Mount(&'a Path, &'a Path),
    Home(&'a Path),
    /// Source of the `/` mount, when known.
    Root(Option<&'a Path>),
}

fn anchor_for<'a>(
    path: &Path,
    mounts: &'a [(PathBuf, PathBuf)],
    home: Option<&'a Path>,
) -> Anchor<'a> {
    // Last of equally deep matches wins: later mounts stack over earlier ones.
    let visible = mounts
        .iter()
        .filter(|(mount_point, _)| {
            path.starts_with(mount_point) && is_user_visible_mount(mount_point, home)
        })
        .max_by_key(|(mount_point, _)| mount_point.components().count());
    if let Some((mount_point, source)) = visible {
        return Anchor::Mount(mount_point, source);
    }
    if let Some(home) = home.filter(|home| path.starts_with(home)) {
        return Anchor::Home(home);
    }
    let root = mounts
        .iter()
        .rev()
        .find(|(mount_point, _)| mount_point == Path::new("/"));
    Anchor::Root(root.map(|(_, source)| source.as_path()))
}

/// GIO's `g_unix_mount_guess_should_display`: removable media under
/// `/media` or `/run/media`, or a mount inside Home, unless hidden behind a
/// dot directory. System mounts (NFS automounts, subvolumes, `/run`) stay
/// part of the root filesystem's path.
fn is_user_visible_mount(mount_point: &Path, home: Option<&Path>) -> bool {
    let below = |base: &str| mount_point.starts_with(base) && mount_point != Path::new(base);
    if mount_point.to_string_lossy().contains("/.") {
        return false;
    }
    below("/media")
        || below("/run/media")
        || home.is_some_and(|home| mount_point.starts_with(home) && mount_point != home)
}

/// `(mount point, mount source)` pairs from `/proc/self/mountinfo`.
#[cfg(target_os = "linux")]
fn read_mounts() -> Vec<(PathBuf, PathBuf)> {
    std::fs::read_to_string("/proc/self/mountinfo")
        .map(|mountinfo| mountinfo.lines().filter_map(parse_mountinfo_line).collect())
        .unwrap_or_default()
}

#[cfg(not(target_os = "linux"))]
fn read_mounts() -> Vec<(PathBuf, PathBuf)> {
    Vec::new()
}

/// The source follows the ` - <fstype>` separator after the optional fields.
#[cfg(target_os = "linux")]
fn parse_mountinfo_line(line: &str) -> Option<(PathBuf, PathBuf)> {
    use std::os::unix::ffi::OsStringExt;
    let mut fields = line.split(' ');
    let mount_point = fields.nth(4)?;
    let source = fields.skip_while(|field| *field != "-").nth(2)?;
    let decode = |value| {
        PathBuf::from(std::ffi::OsString::from_vec(decode_escapes(
            value, "\\", 3, 8,
        )))
    };
    Some((decode(mount_point), decode(source)))
}

/// udev's `/dev/disk/by-label` name for the block device behind `source`.
#[cfg(target_os = "linux")]
fn device_label(source: &Path) -> Option<String> {
    let device = std::fs::canonicalize(source).ok()?;
    std::fs::read_dir("/dev/disk/by-label")
        .ok()?
        .flatten()
        .find(|entry| std::fs::canonicalize(entry.path()).is_ok_and(|target| target == device))
        .map(|entry| {
            let name = entry.file_name();
            String::from_utf8_lossy(&decode_escapes(&name.to_string_lossy(), "\\x", 2, 16))
                .into_owned()
        })
}

#[cfg(not(target_os = "linux"))]
fn device_label(_source: &Path) -> Option<String> {
    None
}

/// `NAME` from os-release, for an unlabelled root filesystem.
fn os_name() -> Option<String> {
    let release = std::fs::read_to_string("/etc/os-release").ok()?;
    release
        .lines()
        .find_map(|line| line.strip_prefix("NAME="))
        .map(|name| name.trim_matches('"').to_owned())
        .filter(|name| !name.is_empty())
}

/// Decode `<marker><digits>` byte escapes: mountinfo writes `\040` (octal),
/// udev writes `\x20` (hex). Malformed escapes pass through literally.
#[cfg(target_os = "linux")]
fn decode_escapes(value: &str, marker: &str, digits: usize, radix: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len());
    let mut rest = value;
    while let Some(index) = rest.find(marker) {
        let (before, escape) = rest.split_at(index);
        out.extend_from_slice(before.as_bytes());
        let code = escape
            .get(marker.len()..marker.len() + digits)
            .filter(|code| code.chars().all(|c| c.is_digit(radix)))
            .and_then(|code| u8::from_str_radix(code, radix).ok());
        match code {
            Some(byte) => {
                out.push(byte);
                rest = &escape[marker.len() + digits..];
            }
            None => {
                out.extend_from_slice(marker.as_bytes());
                rest = &escape[marker.len()..];
            }
        }
    }
    out.extend_from_slice(rest.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_root_skips_system_mounts_and_prefers_deepest_visible_place() {
        let mounts: Vec<(PathBuf, PathBuf)> = [
            ("/", "/dev/root"),
            ("/nix/store", "/dev/root"),
            ("/repo", "nimbus:/repo"),
            ("/media/usb", "/dev/sdb1"),
            ("/home/toph/data", "/dev/sdc1"),
            ("/home/toph/.cache", "tmpfs"),
        ]
        .into_iter()
        .map(|(mount_point, source)| (PathBuf::from(mount_point), PathBuf::from(source)))
        .collect();
        let home = Some(Path::new("/home/toph"));
        let anchor = |path: &str| anchor_for(Path::new(path), &mounts, home);

        assert_eq!(
            anchor("/repo/Nix"),
            Anchor::Root(Some(Path::new("/dev/root")))
        );
        assert_eq!(
            anchor("/nix/store/abc"),
            Anchor::Root(Some(Path::new("/dev/root")))
        );
        assert_eq!(
            anchor("/media/usb/photos"),
            Anchor::Mount(Path::new("/media/usb"), Path::new("/dev/sdb1"))
        );
        assert_eq!(
            anchor("/home/toph/data/x"),
            Anchor::Mount(Path::new("/home/toph/data"), Path::new("/dev/sdc1"))
        );
        assert_eq!(
            anchor("/home/toph/.cache/x"),
            Anchor::Home(Path::new("/home/toph"))
        );
        assert_eq!(
            anchor("/home/other"),
            Anchor::Root(Some(Path::new("/dev/root")))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mountinfo_line_skips_optional_fields_and_decodes_spaces() {
        let line =
            "36 35 98:0 / /mnt/my\\040disk rw,noatime master:1 shared:7 - ext4 /dev/sd\\040a1 rw";
        assert_eq!(
            parse_mountinfo_line(line),
            Some((PathBuf::from("/mnt/my disk"), PathBuf::from("/dev/sd a1")))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn udev_label_escapes_decode_and_malformed_ones_pass_through() {
        assert_eq!(decode_escapes("My\\x20Disk", "\\x", 2, 16), b"My Disk");
        assert_eq!(decode_escapes("a\\xZZb\\x2", "\\x", 2, 16), b"a\\xZZb\\x2");
    }
}
