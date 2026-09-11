{
  lib,
  pkgs,
  sworm,
  version ? "0.0.0",
  src,
  maintainer ? "Chris Toph <toph@ryot.foo>",
  homepage ? "https://github.com/TophC7/Sworm",
}:
let
  debArch =
    {
      "x86_64-linux" = "amd64";
      "aarch64-linux" = "arm64";
    }
    .${pkgs.stdenv.hostPlatform.system}
      or (throw "Unsupported architecture for deb: ${pkgs.stdenv.hostPlatform.system}");

  # Clean version string suitable for Debian package
  debVersion = builtins.replaceStrings [ "-" ] [ "." ] version;

  closure = pkgs.closureInfo { rootPaths = [ sworm ]; };

  deb = pkgs.stdenv.mkDerivation {
    pname = "sworm-deb";
    inherit version;

    nativeBuildInputs = [
      pkgs.dpkg
      pkgs.gnused
    ];

    dontUnpack = true;

    buildPhase = ''
            runHook preBuild

            pkgdir="$TMPDIR/pkg"
            mkdir -p "$pkgdir/DEBIAN"
            mkdir -p "$pkgdir/usr/bin"
            mkdir -p "$pkgdir/usr/share/applications"
            mkdir -p "$pkgdir/usr/share/icons"
            mkdir -p "$pkgdir/usr/share/doc/sworm"
            mkdir -p "$pkgdir/nix/store"

            # 1. Debian control file
            cat > "$pkgdir/DEBIAN/control" <<EOF
      Package: sworm
      Version: ${debVersion}
      Section: devel
      Priority: optional
      Architecture: ${debArch}
      Maintainer: ${maintainer}
      Homepage: ${homepage}
      Depends: git
      Conflicts: nix
      Description: Linux-first desktop app for coding-agent CLIs
       Sworm is a Linux-first desktop development environment for working with coding-agent CLIs.
      EOF

            # 2. Populate complete runtime closure so it runs standalone on any Debian/Ubuntu host
            while IFS= read -r storePath; do
              cp -a "$storePath" "$pkgdir/nix/store/"
            done < "${closure}/store-paths"

            # 3. Launcher in /usr/bin
            cat > "$pkgdir/usr/bin/sworm" <<'EOF'
      #!/bin/sh
      exec ${sworm}/bin/sworm "$@"
      EOF
            chmod 0755 "$pkgdir/usr/bin/sworm"

            # 4. Desktop entry and icons
            if [ -f "${sworm}/share/applications/sworm.desktop" ]; then
              cp "${sworm}/share/applications/sworm.desktop" "$pkgdir/usr/share/applications/"
              sed -i 's|^Exec=sworm|Exec=/usr/bin/sworm|' "$pkgdir/usr/share/applications/sworm.desktop"
            fi

            if [ -d "${sworm}/share/icons" ]; then
              cp -r "${sworm}/share/icons/"* "$pkgdir/usr/share/icons/"
            fi
            # 5. License
            mkdir -p "$pkgdir/usr/share/doc/sworm"
            install -Dm644 ${src}/LICENSE "$pkgdir/usr/share/doc/sworm/LICENSE"

            # 6. Build the deb package
            mkdir -p "$out"
            debFileName="sworm_${debVersion}_${debArch}.deb"
            dpkg-deb --build --root-owner-group --threads-max=0 -Zzstd "$pkgdir" "$out/$debFileName"

            runHook postBuild
    '';

    meta = {
      description = "Self-contained Debian package for Sworm desktop";
      license = lib.licenses.agpl3Plus;
      platforms = lib.platforms.linux;
    };
  };

  pkgbuildTemplate = ''
    pkgname=sworm-bin
    pkgver=@VERSION@
    pkgrel=1
    pkgdesc='Linux-first desktop app for coding-agent CLIs'
    arch=('x86_64' 'aarch64')
    url='${homepage}'
    license=('AGPL-3.0-or-later')
    depends=(
      'gcc-libs'
      'git'
      'glib2'
      'glibc'
      'gtk3'
      'webkit2gtk-4.1'
    )
    makedepends=('libarchive')
    provides=("sworm=$pkgver")
    conflicts=('sworm' 'nix')
    options=('!strip')
    source_x86_64=("https://github.com/TophC7/Sworm/releases/download/@COMMIT@/sworm_''${pkgver}_amd64.deb")
    sha256sums_x86_64=('@AMD64_SHA256@')
    source_aarch64=("https://github.com/TophC7/Sworm/releases/download/@COMMIT@/sworm_''${pkgver}_arm64.deb")
    sha256sums_aarch64=('@ARM64_SHA256@')
    noextract=("sworm_''${pkgver}_amd64.deb" "sworm_''${pkgver}_arm64.deb")

    package() {
      local deb_arch
      case "$CARCH" in
        x86_64) deb_arch=amd64 ;;
        aarch64) deb_arch=arm64 ;;
      esac

      bsdtar -xOf "$srcdir/sworm_''${pkgver}_''${deb_arch}.deb" 'data.tar.*' |
        bsdtar -xf - -C "$pkgdir"

      install -Dm644 "$pkgdir/usr/share/doc/sworm/LICENSE" \
        "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
      rm -f "$pkgdir/usr/share/doc/sworm/LICENSE"
      rmdir --ignore-fail-on-non-empty "$pkgdir/usr/share/doc/sworm" "$pkgdir/usr/share/doc"
    }
  '';

  generate-aur = pkgs.writeShellScriptBin "generate-aur" ''
        set -euo pipefail
        export PATH="${pkgs.pacman}/bin:${pkgs.coreutils}/bin:${pkgs.gnused}/bin:${pkgs.gnutar}/bin:$PATH"

        if [ "$#" -ne 2 ]; then
          echo "Usage: generate-aur <package-version> <dist-directory>" >&2
          exit 1
        fi

        RAW_VERSION="$1"
        VERSION="''${RAW_VERSION//-/.}"
        DIST_DIR="$2"
        COMMIT_HASH="''${RAW_VERSION#*+}"

        case "$RAW_VERSION" in
          *+*) ;;
          *)
            echo "Error: Version must match <version>+<commit-hash>, got: $RAW_VERSION" >&2
            exit 1
            ;;
        esac

        AMD64_DEB="$DIST_DIR/sworm_''${VERSION}_amd64.deb"
        ARM64_DEB="$DIST_DIR/sworm_''${VERSION}_arm64.deb"

        if [ ! -f "$AMD64_DEB" ] || [ ! -f "$ARM64_DEB" ]; then
          echo "Missing release artifacts in $DIST_DIR: $AMD64_DEB or $ARM64_DEB" >&2
          exit 1
        fi

        AMD64_SHA256=$(sha256sum "$AMD64_DEB" | cut -d' ' -f1)
        ARM64_SHA256=$(sha256sum "$ARM64_DEB" | cut -d' ' -f1)

        WORK_DIR="$DIST_DIR/aur"
        mkdir -p "$WORK_DIR"

        cat > "$WORK_DIR/PKGBUILD" <<'EOF'
    ${pkgbuildTemplate}
    EOF

        sed -i \
          -e "s|@VERSION@|$VERSION|g" \
          -e "s|@COMMIT@|$COMMIT_HASH|g" \
          -e "s|@AMD64_SHA256@|$AMD64_SHA256|g" \
          -e "s|@ARM64_SHA256@|$ARM64_SHA256|g" \
          "$WORK_DIR/PKGBUILD"

        CONF="${pkgs.pacman}/etc/makepkg.conf"

        (
          cd "$WORK_DIR"
          makepkg --config "$CONF" --printsrcinfo > .SRCINFO
        )

        tar -czf "$DIST_DIR/sworm-bin-$COMMIT_HASH.tar.gz" -C "$WORK_DIR" PKGBUILD .SRCINFO
        echo "Generated $DIST_DIR/sworm-bin-$COMMIT_HASH.tar.gz"
  '';
in
{
  inherit deb generate-aur;
}
