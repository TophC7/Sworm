{
  description = "ADE - Linux-first desktop app for coding-agent CLIs";

  nixConfig = {
    extra-substituters = [
      "https://cache.nixos.org"
      "https://nix-community.cachix.org"
    ];
    extra-trusted-public-keys = [
      "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY="
      "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    bun2nix.url = "github:nix-community/bun2nix";
    bun2nix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      bun2nix,
    }:
    let
      lib = nixpkgs.lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = lib.genAttrs systems;
      pkgsFor = system: import nixpkgs { inherit system; };
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          b2n = bun2nix.packages.${system}.default;
          fs = pkgs.lib.fileset;
          hasPackageJson = builtins.pathExists ./package.json;
          hasBunLock = builtins.pathExists ./bun.lock;
          hasBunNix = builtins.pathExists ./bun.nix;
          hasFrontendSrc = builtins.pathExists ./src;
          hasTauriSrc = builtins.pathExists ./src-tauri;
          hasTauriConfig = builtins.pathExists ./src-tauri/tauri.conf.json;

          runtimeLibraries = with pkgs; [
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
            xdotool
          ];

          docsPackage = pkgs.writeTextDir "share/doc/ade/README.txt" ''
            ADE bootstrap flake

            This package exists so `nix build` works before the Tauri app scaffold is present.

            Current purpose:
            - provide a valid default package for the repository
            - keep the flake usable during the planning/bootstrap phase

            Once the application scaffold exists, the default package switches to the real app-facing derivation.
          '';

          appPackage =
            let
              sourceFiles = fs.unions (
                [
                  ./package.json
                  ./bun.lock
                  ./bun.nix
                  ./svelte.config.js
                  ./vite.config.ts
                ]
                ++ lib.optional (builtins.pathExists ./tsconfig.json) ./tsconfig.json
                ++ lib.optional (builtins.pathExists ./src) ./src
                ++ lib.optional (builtins.pathExists ./src-tauri) ./src-tauri
              );
            in
            pkgs.stdenv.mkDerivation {
              pname = "ade";
              version = "0.1.0";

              src = fs.toSource {
                root = ./.;
                fileset = sourceFiles;
              };

              nativeBuildInputs = [
                b2n.hook
                pkgs.bun
                pkgs.clang
                pkgs.pkg-config
              ];

              buildInputs = runtimeLibraries;

              bunDeps = b2n.fetchBunDeps {
                bunNix = ./bun.nix;
              };

              # bun2nix's default build target is not what we want for a Tauri/SvelteKit app.
              dontUseBunBuild = true;
              dontUseBunCheck = true;
              dontUseBunInstall = true;

              buildPhase = ''
                runHook preBuild

                export HOME="$TMPDIR"
                export GIO_MODULE_DIR="${pkgs.glib-networking}/lib/gio/modules"
                export XDG_DATA_DIRS="${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:${pkgs.shared-mime-info}/share"
                export LD_LIBRARY_PATH="${lib.makeLibraryPath runtimeLibraries}"
                export LIBCLANG_PATH="${pkgs.llvmPackages.libclang.lib}/lib"

                bun run build

                runHook postBuild
              '';

              installPhase = ''
                runHook preInstall

                mkdir -p $out/share/ade
                cp -r build $out/share/ade/frontend
                cp -r src-tauri $out/share/ade/src-tauri

                runHook postInstall
              '';

              meta = {
                description = "ADE - Linux-first desktop app for coding-agent CLIs";
                license = pkgs.lib.licenses.mit;
                platforms = pkgs.lib.platforms.linux;
              };
            };
        in
        {
          docs = docsPackage;
          default =
            if hasPackageJson && hasBunLock && hasBunNix && hasFrontendSrc && hasTauriSrc && hasTauriConfig then
              appPackage
            else
              docsPackage;
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          b2n = bun2nix.packages.${system}.default;
          runtimeLibraries = with pkgs; [
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
            xdotool
          ];
        in
        {
          default = pkgs.mkShell {
            name = "ade-dev-shell";

            nativeBuildInputs = [
              pkgs.clang
              pkgs.pkg-config
            ];

            buildInputs = runtimeLibraries;

            packages = [
              b2n
              pkgs.bun
              pkgs.cargo
              pkgs.clippy
              pkgs.curl
              pkgs.file
              pkgs.forgejo-mcp
              pkgs.git
              pkgs.gsettings-desktop-schemas
              pkgs.jq
              pkgs.nixfmt-classic
              pkgs.openssl
              pkgs.ripgrep
              pkgs.rust-analyzer
              pkgs.rustc
              pkgs.rustfmt
              pkgs.shared-mime-info
              pkgs.wget
            ];

            LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibraries;
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

            shellHook = ''
              export GIO_MODULE_DIR="${pkgs.glib-networking}/lib/gio/modules"
              export XDG_DATA_DIRS="${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:${pkgs.shared-mime-info}/share''${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
              export BUN_INSTALL="$PWD/.bun"
            '';
          };
        }
      );

      formatter = forAllSystems (system: (pkgsFor system).nixfmt-classic);
    };
}
