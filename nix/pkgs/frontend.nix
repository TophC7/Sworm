{
  lib,
  pkgs,
  b2n,
  src,
}:
let
  fs = pkgs.lib.fileset;
in
pkgs.stdenv.mkDerivation {
  name = "sworm-frontend";

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
    bun run build
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p $out
    cp -r build/* $out/
    runHook postInstall
  '';

  meta = {
    description = "Sworm frontend (SvelteKit SPA)";
    license = lib.licenses.agpl3Plus;
    platforms = lib.platforms.linux;
  };
}
