{
  lib,
  pkgs,
  sworm-server,
  version,
  src,
  maintainer ? "Chris Toph <toph@ryot.foo>",
  homepage ? "https://github.com/TophC7/Sworm",
}:
let
  cleanVersion = builtins.replaceStrings [ "-" ] [ "." ] version;
  debArch =
    {
      "x86_64-linux" = "amd64";
      "aarch64-linux" = "arm64";
    }
    .${pkgs.stdenv.hostPlatform.system}
      or (throw "Unsupported architecture for deb: ${pkgs.stdenv.hostPlatform.system}");

  tarArch =
    {
      "x86_64-linux" = "x86_64";
      "aarch64-linux" = "aarch64";
    }
    .${pkgs.stdenv.hostPlatform.system}
      or (throw "Unsupported architecture for tarball: ${pkgs.stdenv.hostPlatform.system}");

  tarBaseName = "sworm-server-${cleanVersion}-linux-${tarArch}";

  # Reusable relocatable bundle containing binary, libraries, launcher and unit
  bundle = pkgs.stdenv.mkDerivation {
    pname = "sworm-server-bundle";
    version = cleanVersion;

    nativeBuildInputs = [ pkgs.patchelf ];
    dontUnpack = true;

    buildPhase = ''
            runHook preBuild

            mkdir -p "$out/bin" "$out/lib" "$out/share/systemd/user" "$out/share/doc/sworm-server"

            # 1. Stripped raw binary
            cp "${sworm-server}/bin/sworm-server" "$out/bin/.sworm-server-bin"
            chmod +w "$out/bin/.sworm-server-bin"
            ${pkgs.stdenv.cc.targetPrefix}strip "$out/bin/.sworm-server-bin"
            patchelf --set-rpath '$ORIGIN/../lib' "$out/bin/.sworm-server-bin"

            # 2. Gather dynamic runtime libraries (glibc and libgcc)
            for lib in $(patchelf --print-needed "$out/bin/.sworm-server-bin"); do
              found=$(find "${pkgs.glibc}/lib" "${pkgs.stdenv.cc.cc.lib}/lib" -maxdepth 1 -name "$lib" | head -n 1)
              if [ -n "$found" ]; then
                cp -L "$found" "$out/lib/"
              fi
            done
            find "${pkgs.glibc}/lib" -maxdepth 1 -name 'ld-linux*' -exec cp -L {} "$out/lib/" \;
            ldSoName="$(basename "$(find "$out/lib" -maxdepth 1 -name 'ld-linux*' | head -n 1)")"

            # 3. Wrapper script for relocatable standalone execution
            cat > "$out/bin/sworm-server" <<EOF
      #!/bin/sh
      set -e
      BIN_DIR="\$(cd "\$(dirname "\$0")" && pwd)"
      BASE_DIR="\$(cd "\$BIN_DIR/.." && pwd)"
      LIB_DIR="\$BASE_DIR/lib"
      exec "\$LIB_DIR/$ldSoName" --inhibit-cache --library-path "\$LIB_DIR" --argv0 sworm-server "\$BIN_DIR/.sworm-server-bin" "\$@"
      EOF
            chmod +x "$out/bin/sworm-server"

            # 4. Systemd user service
            cat > "$out/share/systemd/user/sworm-server.service" <<'EOF'
      [Unit]
      Description=Sworm remote workspace server
      After=network.target

      [Service]
      ExecStart=/usr/bin/sworm-server serve --listen 0.0.0.0:7420
      Restart=on-failure
      KillMode=mixed

      [Install]
      WantedBy=default.target
      EOF

            # 5. Documentation and License
            install -Dm644 "${src}/LICENSE" "$out/share/doc/sworm-server/LICENSE"

            runHook postBuild
    '';

    meta = {
      description = "Self-contained relocatable bundle for Sworm server";
      license = lib.licenses.agpl3Plus;
      platforms = lib.platforms.linux;
    };
  };

  deb = pkgs.stdenv.mkDerivation {
    pname = "sworm-server-deb";
    inherit version;
    nativeBuildInputs = [ pkgs.dpkg ];
    dontUnpack = true;

    buildPhase = ''
            runHook preBuild

            pkgdir="$TMPDIR/pkg"
            mkdir -p "$pkgdir/DEBIAN" "$pkgdir/usr/bin" "$pkgdir/usr/lib/sworm-server/bin" "$pkgdir/usr/lib/sworm-server/lib" "$pkgdir/usr/lib/systemd/user" "$pkgdir/usr/share/doc/sworm-server"

            cat > "$pkgdir/DEBIAN/control" <<EOF
      Package: sworm-server
      Version: ${cleanVersion}
      Section: devel
      Priority: optional
      Architecture: ${debArch}
      Maintainer: ${maintainer}
      Homepage: ${homepage}
      Depends: git
      Description: Headless Sworm daemon for remote workspaces
       Sworm server is a headless daemon for running coding-agent CLIs in remote workspaces.
      EOF

            mkdir -p "$pkgdir/usr/lib/sworm-server/bin" "$pkgdir/usr/lib/sworm-server/lib" "$pkgdir/usr/bin"
            cp -r "${bundle}/bin/"* "$pkgdir/usr/lib/sworm-server/bin/"
            cp -r "${bundle}/lib/"* "$pkgdir/usr/lib/sworm-server/lib/"

            cat > "$pkgdir/usr/bin/sworm-server" <<'EOF'
      #!/bin/sh
      exec /usr/lib/sworm-server/bin/sworm-server "$@"
      EOF
            chmod +x "$pkgdir/usr/bin/sworm-server"
            cp "${bundle}/share/systemd/user/sworm-server.service" "$pkgdir/usr/lib/systemd/user/"
            cp "${bundle}/share/doc/sworm-server/LICENSE" "$pkgdir/usr/share/doc/sworm-server/"

            mkdir -p "$out"
            dpkg-deb --build --root-owner-group --threads-max=0 -Zzstd "$pkgdir" "$out/sworm-server_${cleanVersion}_${debArch}.deb"

            runHook postBuild
    '';

    meta = {
      description = "Self-contained Debian package for Sworm server";
      license = lib.licenses.agpl3Plus;
      platforms = lib.platforms.linux;
    };
  };

  tarball = pkgs.stdenv.mkDerivation {
    pname = "sworm-server-tarball";
    inherit version;
    dontUnpack = true;

    buildPhase = ''
            runHook preBuild

            workdir="$TMPDIR/${tarBaseName}"
            mkdir -p "$workdir"
            cp -r "${bundle}/"* "$workdir/"
            ln -s share/doc/sworm-server/LICENSE "$workdir/LICENSE"

            cat > "$workdir/install.sh" <<'EOF'
      #!/bin/sh
      set -e
      PREFIX="''${PREFIX:-/usr/local}"
      echo "Installing sworm-server to $PREFIX..."

      SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
      BIN_DIR="$PREFIX/bin"
      LIB_DIR="$PREFIX/lib/sworm-server"

      mkdir -p "$BIN_DIR" "$LIB_DIR/bin" "$LIB_DIR/lib"
      cp -r "$SCRIPT_DIR/bin/"* "$LIB_DIR/bin/"
      cp -r "$SCRIPT_DIR/lib/"* "$LIB_DIR/lib/"

      cat > "$BIN_DIR/sworm-server" <<INNER
      #!/bin/sh
      exec "$LIB_DIR/bin/sworm-server" "\$@"
      INNER
      chmod +x "$BIN_DIR/sworm-server"

      if [ "$(id -u)" -eq 0 ] && [ "$PREFIX" = "/usr" ]; then
        SYSTEMD_DIR="/usr/lib/systemd/user"
      else
        SYSTEMD_DIR="''${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
      fi
      if mkdir -p "$SYSTEMD_DIR" 2>/dev/null && sed "s|/usr/bin/sworm-server|$BIN_DIR/sworm-server|g" "$SCRIPT_DIR/share/systemd/user/sworm-server.service" > "$SYSTEMD_DIR/sworm-server.service" 2>/dev/null; then
        echo "Installed systemd unit to $SYSTEMD_DIR/sworm-server.service"
      else
        echo "Notice: skipped systemd user service installation (could not write to $SYSTEMD_DIR)"
      fi
      echo "Successfully installed sworm-server to $BIN_DIR/sworm-server"
      EOF
            chmod +x "$workdir/install.sh"

            mkdir -p "$out"
            tar -czf "$out/${tarBaseName}.tar.gz" -C "$TMPDIR" "${tarBaseName}"

            runHook postBuild
    '';

    meta = {
      description = "Standalone portable tarball for Sworm server";
      license = lib.licenses.agpl3Plus;
      platforms = lib.platforms.linux;
    };
  };
in
{
  inherit bundle deb tarball;
}
