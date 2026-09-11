{
  lib,
  pkgs,
  craneLib,
  frontend,
  version,
  runtimeLibraries,
  workspaceSourceFilter,
  src,
}:
let
  desktopFile = src + "/src-tauri/sworm.desktop";

  commonArgs = {
    pname = "sworm";
    inherit version;
    src = lib.cleanSourceWith {
      inherit src;
      filter = workspaceSourceFilter craneLib;
    };

    doCheck = false;
    strictDeps = true;

    nativeBuildInputs = with pkgs; [
      clang
      pkg-config
      wrapGAppsHook3
    ];

    buildInputs = runtimeLibraries;

    # cargo test spawns `git` from services::git tests; sandbox PATH
    # is empty otherwise. Scoped to checkPhase to keep dep cache lean.
    nativeCheckInputs = [ pkgs.git ];

    # Tauri's mock windows still create their app-local data directory.
    preCheck = ''
      export HOME="$TMPDIR"
      export XDG_DATA_HOME="$TMPDIR/.local/share"
    '';

    # The tauri CLI injects this feature automatically; raw cargo does not.
    # Without it, cfg(dev) stays active and assets are not embedded.
    cargoExtraArgs = "--locked -p sworm --features tauri/custom-protocol";
    cargoTestExtraArgs = "-p sworm-core -p sworm-protocol -p sworm-remote -p sworm-server -p sworm --features tauri/custom-protocol";

    LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
    TAURI_CONFIG = builtins.toJSON { inherit version; };
  };

  cargoArtifacts = craneLib.buildDepsOnly (
    (builtins.removeAttrs commonArgs [
      "version"
      "TAURI_CONFIG"
    ])
    // {
      version = (builtins.fromTOML (builtins.readFile (src + "/src-tauri/Cargo.toml"))).package.version;
      preBuild = ''
        mkdir -p build
        echo '<html></html>' > build/index.html
      '';
    }
  );
in
craneLib.buildPackage (
  commonArgs
  // {
    inherit cargoArtifacts;

    preBuild = ''
      mkdir -p build
      cp -r ${frontend}/* build/
    '';

    postInstall = ''
      install -Dm644 src-tauri/icons/128x128.png $out/share/icons/hicolor/128x128/apps/sworm.png
      install -Dm644 ${desktopFile} $out/share/applications/sworm.desktop
    '';

    meta = {
      description = "Sworm - Linux-first desktop app for coding-agent CLIs";
      license = lib.licenses.agpl3Plus;
      platforms = lib.platforms.linux;
      mainProgram = "sworm";
    };
  }
)
