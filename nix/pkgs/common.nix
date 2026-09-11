{ lib }:
{
  # Runtime libraries needed by WebKitGTK/Tauri at both build and run time.
  runtimeLibsFor =
    pkgs: with pkgs; [
      atk
      cairo
      dbus
      gdk-pixbuf
      glib
      glib-networking
      gtk3
      libayatana-appindicator
      librsvg
      libsoup_3
      openssl
      pango
      webkitgtk_4_1
    ];

  # Filter for crane workspace sources: keep Rust source, Tauri configs, desktop files, migrations, icons, and builtins.
  workspaceSourceFilter =
    craneLib: path: type:
    (craneLib.filterCargoSources path type)
    || (lib.hasSuffix "tauri.conf.json" path)
    || (lib.hasSuffix "sworm.desktop" path)
    || (lib.hasInfix "src-crates/sworm-core/builtins" path)
    || (lib.hasInfix "src-tauri/capabilities" path)
    || (lib.hasInfix "src-tauri/icons" path)
    || (lib.hasInfix "src-crates/sworm-core/migrations" path);
}
