{ self }:
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
}
