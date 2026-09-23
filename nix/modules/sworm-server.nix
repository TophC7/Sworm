{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.sworm-server;
  home = config.users.users.${cfg.user}.home;
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

    user = lib.mkOption {
      type = lib.types.strMatching "[a-z_][a-z0-9_-]*[$]?";
      description = "Existing user account that owns the server identity and data.";
    };

    listen = lib.mkOption {
      type = lib.types.str;
      default = "0.0.0.0:7420";
      description = "UDP address and port on which the Sworm server listens.";
    };

    authTokenFile = lib.mkOption {
      type = lib.types.nullOr (lib.types.strMatching "/.+");
      default = null;
      description = "Absolute path to a runtime-readable token file; its contents never enter the Nix store.";
    };

    xdgConfigHome = lib.mkOption {
      type = lib.types.nullOr (lib.types.strMatching "/.+");
      default = null;
      description = "XDG config home for the server; use the same path as the user's shell when customized.";
    };

    xdgDataHome = lib.mkOption {
      type = lib.types.nullOr (lib.types.strMatching "/.+");
      default = null;
      description = "XDG data home for the server; use the same path as the user's shell when customized.";
    };

    extraPackages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
      description = "Additional commands available to server-side workspaces and tasks.";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Whether to open the configured UDP port in the firewall.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = builtins.hasAttr cfg.user config.users.users;
        message = "services.sworm-server.user must name a configured NixOS user.";
      }
    ];
    environment.systemPackages = [ cfg.package ];
    networking.firewall.allowedUDPPorts = lib.mkIf cfg.openFirewall [ listenPort ];

    systemd.services.sworm-server = {
      description = "Sworm remote workspace server";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      path = [
        pkgs.git
        pkgs.nix
        pkgs.openssh
        pkgs.fish
        pkgs.bash
        pkgs.coreutils
      ]
      ++ cfg.extraPackages;
      environment = {
        HOME = home;
      }
      // lib.optionalAttrs (cfg.xdgConfigHome != null) {
        XDG_CONFIG_HOME = cfg.xdgConfigHome;
      }
      // lib.optionalAttrs (cfg.xdgDataHome != null) {
        XDG_DATA_HOME = cfg.xdgDataHome;
      }
      // lib.optionalAttrs (cfg.authTokenFile != null) {
        SWORM_SERVER_AUTH_TOKEN_FILE = cfg.authTokenFile;
      };
      serviceConfig = {
        User = cfg.user;
        WorkingDirectory = home;
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
}
