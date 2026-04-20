{
  description = "Sworm - Linux-first Agentic Development Environment";

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

    crane.url = "github:ipetkov/crane";

    bun2nix = {
      url = "github:nix-community/bun2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      bun2nix,
    }:
    let
      lib = nixpkgs.lib;
      version = "0.1.0";
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = lib.genAttrs systems;
      pkgsFor = system: import nixpkgs { inherit system; };

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
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          b2n = bun2nix.packages.${system}.default;
          craneLib = crane.mkLib pkgs;
          fs = pkgs.lib.fileset;
          runtimeLibraries = runtimeLibsFor pkgs;

          # Frontend-only build (SvelteKit via bun).
          frontend = pkgs.stdenv.mkDerivation {
            pname = "sworm-frontend";
            inherit version;

            src = fs.toSource {
              root = ./.;
              fileset = fs.unions [
                ./package.json
                ./bun.lock
                ./bun.nix
                ./svelte.config.js
                ./vite.config.ts
                ./tsconfig.json
                ./src
                ./static
              ];
            };

            nativeBuildInputs = [
              b2n.hook
              pkgs.bun
            ];

            bunDeps = b2n.fetchBunDeps {
              bunNix = ./bun.nix;
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
          };

          # Shared args for crane's dep and source builds.
          # src-tauri IS the Cargo root, so use it directly as the source.
          # cleanCargoSource strips non-Cargo files; we extend the filter
          # to keep Tauri assets (config, capabilities, icons, migrations)
          # that build.rs and the runtime need.
          tauriSourceFilter =
            path: type:
            (craneLib.filterCargoSources path type)
            || (lib.hasSuffix "tauri.conf.json" path)
            || (lib.hasInfix "/capabilities/" path)
            || (lib.hasInfix "/icons/" path)
            || (lib.hasInfix "/migrations/" path);

          commonArgs = {
            pname = "sworm";
            inherit version;
            src = lib.cleanSourceWith {
              src = ./src-tauri;
              filter = tauriSourceFilter;
            };

            strictDeps = true;

            nativeBuildInputs = with pkgs; [
              clang
              pkg-config
              wrapGAppsHook3
            ];

            buildInputs = runtimeLibraries;

            # The tauri CLI injects this feature automatically; raw cargo does not.
            # Without it, cfg(dev) stays active and assets are not embedded.
            cargoExtraArgs = "--features tauri/custom-protocol";

            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          };

          # Phase 1: build only Cargo deps (cached until Cargo.lock changes)
          cargoArtifacts = craneLib.buildDepsOnly (
            commonArgs
            // {
              # Dummy frontend so Tauri's build.rs doesn't fail during dep compilation.
              # frontendDist in tauri.conf.json is "../build" relative to src-tauri.
              preBuild = ''
                mkdir -p $NIX_BUILD_TOP/build
                echo '<html></html>' > $NIX_BUILD_TOP/build/index.html
              '';
            }
          );

          desktopFile = pkgs.writeText "sworm.desktop" ''
            [Desktop Entry]
            Name=Sworm
            Comment=Agentic Development Environment
            Exec=sworm
            Icon=sworm
            Terminal=false
            Type=Application
            Categories=Development;IDE;
          '';

          # Phase 2: build the app against pre-built deps (only recompiles sworm crate)
          appPackage = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;

              # Place the real frontend where Tauri expects it (frontendDist = "../build").
              preBuild = ''
                mkdir -p $NIX_BUILD_TOP/build
                cp -r ${frontend}/* $NIX_BUILD_TOP/build/
              '';

              postInstall = ''
                install -Dm644 icons/128x128.png $out/share/icons/hicolor/128x128/apps/sworm.png
                install -Dm644 ${desktopFile} $out/share/applications/sworm.desktop
              '';

              meta = {
                description = "Sworm - Linux-first desktop app for coding-agent CLIs";
                license = lib.licenses.agpl3Plus;
                platforms = lib.platforms.linux;
                mainProgram = "sworm";
              };
            }
          );
        in
        {
          inherit frontend;
          default = appPackage;
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          b2n = bun2nix.packages.${system}.default;
          runtimeLibraries = runtimeLibsFor pkgs;
        in
        {
          default = pkgs.mkShell {
            name = "sworm-dev-shell";

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
              pkgs.biome
              pkgs.nixfmt
              pkgs.nil
              pkgs.openssl
              pkgs.ripgrep
              pkgs.rust-analyzer
              pkgs.rustc
              pkgs.rustfmt
              pkgs.shared-mime-info
              pkgs.svelte-language-server
              pkgs.tailwindcss-language-server
              pkgs.vtsls
              pkgs.vscode-langservers-extracted
              pkgs.wget
            ];

            LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibraries;
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

            shellHook = ''
              export GIO_MODULE_DIR="${pkgs.glib-networking}/lib/gio/modules"
              export XDG_DATA_DIRS="${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:${pkgs.shared-mime-info}/share''${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
              export BUN_INSTALL="$PWD/.bun"
              git config --local core.hooksPath .githooks
            '';
          };
        }
      );

      formatter = forAllSystems (system: (pkgsFor system).nixfmt);
    };
}
