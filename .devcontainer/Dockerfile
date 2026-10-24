FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# Install GTK 4, Libadwaita, SVG icon loader, icon theme, and native development dependencies
RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
    && apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        libgtk-4-dev \
        libadwaita-1-dev \
        libglib2.0-dev \
        libdbus-1-dev \
        librsvg2-common \
        adwaita-icon-theme \
        fonts-noto-color-emoji \
        dbus-x11 \
        build-essential \
    && rm -rf /var/lib/apt/lists/*
