{
  lib,
  pkgs,
  b2n,
  src,
  target ? "desktop",
}:
assert lib.assertMsg (builtins.elem target [
  "desktop"
  "web"
]) "sworm frontend target must be desktop or web";
let
  fs = pkgs.lib.fileset;
  output = if target == "desktop" then "build" else "build-web";
in
pkgs.stdenv.mkDerivation {
  name = if target == "desktop" then "sworm-frontend" else "sworm-frontend-web";

  src = fs.toSource {
    root = src;
    fileset = fs.unions [
      (src + "/package.json")
      (src + "/bun.lock")
      (src + "/bun.nix")
      (src + "/svelte.config.js")
      (src + "/vite.config.ts")
      (src + "/tsconfig.json")
      (src + "/src")
      (src + "/static")
    ];
  };

  nativeBuildInputs = [
    b2n.hook
    pkgs.bun
  ];

  bunDeps = b2n.fetchBunDeps {
    bunNix = src + "/bun.nix";
  };

  dontUseBunBuild = true;
  dontUseBunCheck = true;
  dontUseBunInstall = true;

  buildPhase = ''
    runHook preBuild
    export HOME="$TMPDIR"
    bun run ${if target == "desktop" then "build" else "build:web"}
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p $out
    cp -r ${output}/* $out/
    runHook postInstall
  '';

  meta = {
    description = "Sworm frontend (SvelteKit SPA)";
    license = lib.licenses.agpl3Plus;
    platforms = lib.platforms.linux;
  };
}
