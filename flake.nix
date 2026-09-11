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
      revision = self.shortRev or self.dirtyShortRev or "dev";
      version = "0.0.0+${revision}";
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

          # Frontend assets do not depend on the app's commit identity.
          frontend = pkgs.stdenv.mkDerivation {
            name = "sworm-frontend";

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
          # Keep workspace sources and the assets embedded by the member crates.
          workspaceSourceFilter =
            path: type:
            (craneLib.filterCargoSources path type)
            || (lib.hasSuffix "tauri.conf.json" path)
            || (lib.hasSuffix "sworm.desktop" path)
            || (lib.hasInfix "src-crates/sworm-core/builtins" path)
            || (lib.hasInfix "src-tauri/capabilities" path)
            || (lib.hasInfix "src-tauri/icons" path)
            || (lib.hasInfix "src-crates/sworm-core/migrations" path);

          commonArgs = {
            pname = "sworm";
            inherit version;
            src = lib.cleanSourceWith {
              src = ./.;
              filter = workspaceSourceFilter;
            };

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

          # Phase 1: build only Cargo deps (cached until Cargo.lock changes)
          cargoArtifacts = craneLib.buildDepsOnly (
            (builtins.removeAttrs commonArgs [
              "version"
              "TAURI_CONFIG"
            ])
            // {
              # Virtual workspace has no package version; keep dependency metadata
              # tied to the desktop manifest, not the checkout revision.
              version = (builtins.fromTOML (builtins.readFile ./src-tauri/Cargo.toml)).package.version;
              # Dummy frontend so Tauri's build.rs doesn't fail during dep compilation.
              # frontendDist in tauri.conf.json is "../build" relative to src-tauri.
              preBuild = ''
                mkdir -p build
                echo '<html></html>' > build/index.html
              '';
            }
          );

          desktopFile = ./src-tauri/sworm.desktop;

          # Build the desktop against cached workspace dependencies.
          appPackage = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;

              # Place the real frontend where Tauri expects it (frontendDist = "../build").
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
          );

          serverArgs = {
            pname = "sworm-server";
            inherit version;
            inherit (commonArgs)
              src
              strictDeps
              nativeCheckInputs
              preCheck
              ;
            cargoExtraArgs = "--locked -p sworm-server";
            cargoTestExtraArgs = "-p sworm-server";
          };

          # Cache only the daemon's Rust dependencies; desktop artifacts include Tauri.
          serverCargoArtifacts = craneLib.buildDepsOnly (
            (builtins.removeAttrs serverArgs [
              "version"
              "nativeCheckInputs"
              "preCheck"
              "cargoTestExtraArgs"
            ])
            // {
              version =
                (builtins.fromTOML (builtins.readFile ./src-crates/sworm-server/Cargo.toml)).package.version;
            }
          );

          serverPackage = craneLib.buildPackage (
            serverArgs
            // {
              cargoArtifacts = serverCargoArtifacts;
              meta = {
                description = "Headless Sworm daemon for remote workspaces";
                license = lib.licenses.agpl3Plus;
                platforms = lib.platforms.linux;
                mainProgram = "sworm-server";
              };
            }
          );
        in
        {
          inherit frontend;
          default = appPackage;
          sworm-server = serverPackage;
        }
      );

      nixosModules.sworm-server =
        {
          config,
          lib,
          pkgs,
          ...
        }:
        let
          cfg = config.services.sworm-server;
          listenPort = lib.toInt (lib.last (lib.splitString ":" cfg.listen));
        in
        {
          options.services.sworm-server = {
            enable = lib.mkEnableOption "Sworm remote workspace server";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.sworm-server;
              defaultText = lib.literalExpression "self.packages.<system>.sworm-server";
              description = "Sworm server package to use.";
            };

            listen = lib.mkOption {
              type = lib.types.str;
              default = "0.0.0.0:7420";
              description = "UDP address and port on which the Sworm server listens.";
            };

            openFirewall = lib.mkOption {
              type = lib.types.bool;
              default = false;
              description = "Whether to open the configured UDP port in the firewall.";
            };
          };

          config = lib.mkIf cfg.enable {
            environment.systemPackages = [ cfg.package ];
            networking.firewall.allowedUDPPorts = lib.mkIf cfg.openFirewall [ listenPort ];

            systemd.user.services.sworm-server = {
              description = "Sworm remote workspace server";
              wantedBy = [ "default.target" ];
              serviceConfig = {
                ExecStart = lib.escapeShellArgs [
                  "${cfg.package}/bin/sworm-server"
                  "serve"
                  "--listen"
                  cfg.listen
                ];
                Restart = "on-failure";
                KillMode = "mixed";
              };
            };
          };
        };

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
              pkgs.fish
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
