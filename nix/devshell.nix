{
  lib,
  pkgs,
  b2n,
  runtimeLibraries,
}:
pkgs.mkShell {
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
}
