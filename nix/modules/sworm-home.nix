{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.sworm;
  settingsDir = "${config.xdg.configHome}/sworm";
in
{
  options.programs.sworm = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Whether to install Sworm when this module is imported.";
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.sworm;
      defaultText = lib.literalExpression "self.packages.<system>.sworm";
      description = "Sworm desktop package to install.";
    };

    settings = lib.mkOption {
      type = lib.types.attrsOf (pkgs.formats.json { }).type;
      default = { };
      description = "Global Sworm settings. Stored in the Nix store; do not put secrets here.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.configFile = lib.mkIf (cfg.settings != { }) {
      "sworm/settings_source" = {
        text = builtins.toJSON cfg.settings;
        onChange = ''
          coreutils="${pkgs.coreutils}/bin"
          settings_dir=${lib.escapeShellArg settingsDir}
          "$coreutils/mkdir" -p "$settings_dir"
          "$coreutils/chmod" 700 "$settings_dir"
          tmp_file="$("$coreutils/mktemp" "$settings_dir/.settings.jsonc.XXXXXX")"
          "$coreutils/cp" "$settings_dir/settings_source" "$tmp_file"
          "$coreutils/chmod" 600 "$tmp_file"
          "$coreutils/mv" -f "$tmp_file" "$settings_dir/settings.jsonc"
        '';
      };
    };
  };
}
