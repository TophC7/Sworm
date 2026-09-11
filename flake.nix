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
      envVersion = builtins.getEnv "APP_VERSION";
      revision = self.shortRev or self.dirtyShortRev or "dev";
      version = if envVersion != "" then envVersion else "0.0.0+${revision}";
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
          craneLib = crane.mkLib pkgs;
          common = import ./nix/pkgs/common.nix { inherit lib; };
          runtimeLibraries = common.runtimeLibsFor pkgs;

          frontend = pkgs.callPackage ./nix/pkgs/frontend.nix {
            inherit b2n;
            src = ./.;
          };

          sworm = pkgs.callPackage ./nix/pkgs/sworm.nix {
            inherit
              craneLib
              frontend
              version
              runtimeLibraries
              ;
            workspaceSourceFilter = common.workspaceSourceFilter;
            src = ./.;
          };

          sworm-server = pkgs.callPackage ./nix/pkgs/sworm-server.nix {
            inherit craneLib version;
            workspaceSourceFilter = common.workspaceSourceFilter;
            src = ./.;
          };

          desktopPackaging = pkgs.callPackage ./nix/packaging/desktop.nix {
            inherit sworm version;
            src = ./.;
          };

          serverPackaging = pkgs.callPackage ./nix/packaging/server.nix {
            inherit sworm-server version;
            src = ./.;
          };
        in
        {
          inherit
            frontend
            sworm
            sworm-server
            ;
          deb = desktopPackaging.deb;
          generate-aur = desktopPackaging.generate-aur;
          server-deb = serverPackaging.deb;
          server-tarball = serverPackaging.tarball;
          default = sworm;
        }
      );

      apps = forAllSystems (system: {
        generate-aur = {
          type = "app";
          program = "${self.packages.${system}.generate-aur}/bin/generate-aur";
        };
      });

      nixosModules.sworm-server = import ./nix/modules/sworm-server.nix { inherit self; };

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          b2n = bun2nix.packages.${system}.default;
          common = import ./nix/pkgs/common.nix { inherit lib; };
          runtimeLibraries = common.runtimeLibsFor pkgs;
        in
        {
          default = pkgs.callPackage ./nix/devshell.nix {
            inherit b2n runtimeLibraries;
          };
        }
      );

      formatter = forAllSystems (system: (pkgsFor system).nixfmt);
    };
}
