# syntax=docker/dockerfile:1

FROM oven/bun:1.3.13 AS bun-deps
WORKDIR /app
COPY package.json bun.lock ./
RUN --mount=type=cache,target=/root/.bun/install/cache \
    bun install --frozen-lockfile --ignore-scripts

FROM rust:1.93.0-bookworm AS build
RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        clang libclang-dev libgtk-3-dev librsvg2-dev libssl-dev \
        libwebkit2gtk-4.1-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
COPY --from=bun-deps /usr/local/bin/bun /usr/local/bin/bun
COPY --from=bun-deps /app/node_modules ./node_modules

ARG APP_VERSION=0.0.0+dev
ARG TARGETARCH
ENV CARGO_TARGET_DIR=/app/.cargo-target
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,id=sworm-cargo-target-${TARGETARCH},target=/app/.cargo-target \
    set -eu; \
    config="$(bun -e 'console.log(JSON.stringify({version: process.env.APP_VERSION}))')"; \
    rm -rf "$CARGO_TARGET_DIR/release/bundle/deb"; \
    bun tauri build --config "$config" --bundles deb -- --locked; \
    arch="$(dpkg --print-architecture)"; \
    case "$arch" in amd64|arm64) ;; *) echo "unsupported Debian architecture: $arch" >&2; exit 1;; esac; \
    set -- "$CARGO_TARGET_DIR"/release/bundle/deb/*.deb; \
    [ "$#" -eq 1 ] && [ -f "$1" ]; \
    [ "$(dpkg-deb --field "$1" Version)" = "$APP_VERSION" ]; \
    install -Dm644 "$1" "/out/sworm_${APP_VERSION}_${arch}.deb"

FROM debian:bookworm-slim AS smoke
COPY --from=build /out/*.deb /tmp/
RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        /tmp/*.deb dbus desktop-file-utils fish xauth xdotool xvfb \
    && rm -rf /var/lib/apt/lists/*
COPY packaging/smoke.fish /smoke.fish
# Docker's namespace restrictions prevent WebKit's nested sandbox. Only this
# disposable smoke stage disables it; the installed app keeps its sandbox.
RUN desktop-file-validate /usr/share/applications/Sworm.desktop \
    && WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 \
       dbus-run-session -- xvfb-run -a fish /smoke.fish

FROM scratch AS artifacts
COPY --from=build /out/ /
