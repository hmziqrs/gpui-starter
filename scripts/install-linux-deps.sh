#!/usr/bin/env bash
#
# Install the Debian/Ubuntu build (and runtime) dependencies for gpui-starter.
#
# GPUI's Linux backend is Wayland/X11 + Vulkan + FreeType/fontconfig. It does
# NOT use WebKit or libzstd (those come from a Tauri template and were
# previously listed here by mistake). `libvulkan-dev` is the build header;
# `libvulkan1` (the loader) is a RUNTIME requirement, so both are installed.
#
# Verified on Ubuntu 24.04. Re-runnable.
set -euo pipefail

if command -v apt-get >/dev/null 2>&1; then
  PKGS=(
    gcc
    g++
    clang
    patchelf
    pkg-config
    libfontconfig-dev
    libfreetype-dev
    libwayland-dev
    wayland-protocols
    libxkbcommon-x11-dev
    libx11-xcb-dev
    libxcb1-dev
    libxcb-render0-dev
    libxcb-shape0-dev
    libxcb-xfixes0-dev
    libssl-dev
    libvulkan-dev
    libvulkan1
  )

  echo "Installing ${#PKGS[@]} packages via apt..."
  sudo apt-get update -y
  sudo apt-get install -y "${PKGS[@]}"
  echo "Done. gpui-starter build dependencies installed."
else
  echo "Error: apt-get not found. This script targets Debian/Ubuntu." >&2
  echo "For Fedora: sudo dnf install gcc-c++ clang pkgconf-pkg-config fontconfig-devel freetype-devel wayland-devel wayland-protocols-devel libxkbcommon-x11-devel libxcb-devel openssl-devel vulkan-headers vulkan-loader" >&2
  echo "For Arch:   sudo pacman -S --needed base-devel clang pkgconf fontconfig freetype2 wayland wayland-protocols libxkbcommon-x11 libxcb openssl vulkan-headers vulkan-icd-loader" >&2
  exit 1
fi
