{
  lib,
  pkgs,
  craneLib,
  version,
  workspaceSourceFilter,
  src,
  webFrontend,
}:
let
  serverArgs = {
    pname = "sworm-server";
    inherit version;
    src = lib.cleanSourceWith {
      inherit src;
      filter = workspaceSourceFilter craneLib;
    };
    strictDeps = true;
    doCheck = false;
    nativeCheckInputs = [ pkgs.git ];
    preCheck = ''
      export HOME="$TMPDIR"
      export XDG_DATA_HOME="$TMPDIR/.local/share"
    '';
    cargoExtraArgs = "--locked -p sworm-server";
    cargoTestExtraArgs = "-p sworm-server";
  };

  serverCargoArtifacts = craneLib.buildDepsOnly (
    (builtins.removeAttrs serverArgs [
      "version"
      "nativeCheckInputs"
      "preCheck"
      "cargoTestExtraArgs"
    ])
    // {
      version =
        (builtins.fromTOML (builtins.readFile (src + "/src-crates/sworm-server/Cargo.toml")))
        .package.version;
    }
  );
  unwrapped = craneLib.buildPackage (
    serverArgs
    // {
      cargoArtifacts = serverCargoArtifacts;
    }
  );
in
pkgs.runCommand "sworm-server-${version}"
  {
    passthru = { inherit unwrapped; };
    meta = {
      description = "Headless Sworm daemon for remote workspaces";
      license = lib.licenses.agpl3Plus;
      platforms = lib.platforms.linux;
      mainProgram = "sworm-server";
    };
  }
  ''
    mkdir -p "$out/bin"
    cat > "$out/bin/sworm-server" <<'EOF'
    #!${pkgs.runtimeShell}
    exec "${unwrapped}/bin/sworm-server" --web-assets-dir "${webFrontend}" "$@"
    EOF
    chmod +x "$out/bin/sworm-server"
  ''
