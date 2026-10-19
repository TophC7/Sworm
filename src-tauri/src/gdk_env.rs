//! GDK environment defaults, applied before GTK initializes.

#[cfg(target_os = "linux")]
use std::path::Path;

/// Prefer native Wayland and apply the NVIDIA GL quirk, preserving explicit
/// user overrides. Returns whether the NVIDIA quirk was applied.
///
/// Called before the Tauri runtime (and its threads) starts.
/// `std::env::set_var` is safe here because no other threads exist yet;
/// it will become `unsafe` in Rust 2024 edition; revisit when upgrading.
#[cfg(target_os = "linux")]
pub(crate) fn configure() -> bool {
    let unset = |name: &str| std::env::var_os(name).is_none_or(|value| value.is_empty());

    if unset("GDK_BACKEND") {
        std::env::set_var("GDK_BACKEND", "wayland,x11");
    }

    // NVIDIA explicit sync rejects GTK3's shm-painted first frame (Wayland
    // Error 71); an up-front paint GL context keeps every frame on GL. tauri#10702
    let gdk_backend = std::env::var("GDK_BACKEND").unwrap_or_default();
    if unset("GDK_GL")
        && uses_wayland(&gdk_backend, !unset("WAYLAND_DISPLAY"))
        && has_nvidia_gpu(Path::new("/sys/class/drm"))
    {
        std::env::set_var("GDK_GL", "always");
        return true;
    }
    false
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn configure() -> bool {
    false
}

/// Whether GDK will pick Wayland: a display exists and Wayland is not ranked
/// after X11 in `GDK_BACKEND`.
#[cfg(target_os = "linux")]
fn uses_wayland(gdk_backend: &str, wayland_display_set: bool) -> bool {
    let first = gdk_backend
        .split(',')
        .map(str::trim)
        .find(|backend| matches!(*backend, "wayland" | "x11"));
    wayland_display_set && first != Some("x11")
}

/// Whether any DRM card is bound to the proprietary `nvidia` driver.
#[cfg(target_os = "linux")]
fn has_nvidia_gpu(drm_root: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(drm_root) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        name.starts_with("card")
            && !name.contains('-')
            && std::fs::read_link(entry.path().join("device/driver"))
                .is_ok_and(|driver| driver.file_name().is_some_and(|name| name == "nvidia"))
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn nvidia_detection_reads_drm_driver_links() {
        let root = tempfile::tempdir().unwrap();
        let card = |name: &str, driver: &str| {
            let device = root.path().join(name).join("device");
            std::fs::create_dir_all(&device).unwrap();
            symlink(format!("../../drivers/{driver}"), device.join("driver")).unwrap();
        };

        card("card0", "amdgpu");
        assert!(!has_nvidia_gpu(root.path()));

        std::fs::create_dir(root.path().join("card1-DP-1")).unwrap();
        card("card1", "nvidia");
        assert!(has_nvidia_gpu(root.path()));

        assert!(!has_nvidia_gpu(&root.path().join("missing")));
    }

    #[test]
    fn wayland_detection_respects_backend_order() {
        assert!(uses_wayland("wayland,x11", true));
        assert!(!uses_wayland("x11,wayland", true));
        assert!(!uses_wayland("x11", true));
        assert!(uses_wayland("*", true));
        assert!(!uses_wayland("wayland,x11", false));
    }
}
