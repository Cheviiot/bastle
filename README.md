# Bastle

![Bastle icon](data/icons/hicolor/scalable/apps/io.github.cheviiot.bastle.svg)

Bastle is a GNOME-native manager for isolated web applications powered by the
system WebKitGTK engine. It creates desktop launchers through the Dynamic
Launcher Portal and keeps each application's metadata, profile, cookies, and
cache separate.

The name *bastle* means a detached fortified house: each website gets its own
small, self-contained space. The friendly shelter icon reflects that idea; the
project is not positioned as a security product.

## Status

Version 0.1 targets x86_64, GNOME 50, and Flatpak. It supports application
creation, editing, launching, deletion, launcher repair, offline creation, and
an explicit settings-only import from Spider. Cookies, browser sessions,
profiles, and cache are never imported.

Flathub submission is intentionally deferred until Bastle has an independent
release history and enough differentiation from Spider.

## Data layout

```text
$XDG_DATA_HOME/bastle/apps/<id>/app.json
$XDG_DATA_HOME/bastle/apps/<id>/icon.png
$XDG_DATA_HOME/bastle/profiles/<id>/
$XDG_CACHE_HOME/bastle/<id>/
```

GSettings stores only the main-window size and whether the first-run legacy
import prompt has been handled. Bastle reads Spider data only after the user
selects a legacy keyfile or configuration directory.

## Development environment

Development is reproducible in the Fedora 44 Distrobox named `bastle-dev`:

```sh
distrobox create --name bastle-dev --image registry.fedoraproject.org/fedora:44 --yes
distrobox enter bastle-dev -- sudo dnf install -y rust cargo rustfmt clippy gcc pkgconf-pkg-config meson ninja-build blueprint-compiler gtk4-devel libadwaita-devel webkitgtk6.0-devel openssl-devel appstream desktop-file-utils flatpak-builder gettext glib2-devel librsvg2-tools xorg-x11-server-Xvfb git
distrobox enter bastle-dev -- meson setup build
distrobox enter bastle-dev -- meson compile -C build
```

To build the Flatpak from inside Distrobox (where FUSE mounts are intentionally
restricted), use:

```sh
distrobox enter bastle-dev -- flatpak-builder --disable-rofiles-fuse --user --force-clean --install-deps-from=flathub .flatpak-build build-aux/io.github.cheviiot.bastle.Devel.json
```

GUI smoke tests must use a dedicated virtual display and explicitly disable
the Wayland socket so Flatpak cannot fall back to the active desktop session:

```sh
distrobox enter bastle-dev -- env -u WAYLAND_DISPLAY xvfb-run -a timeout 10s flatpak run --nosocket=wayland --socket=x11 --env=GDK_BACKEND=x11 io.github.cheviiot.bastle
```

Project-specific system packages are not installed on the ALT Workstation
host. The Flatpak manifest uses GNOME runtime 50 and the Rust SDK extension.

## Provenance and license

Bastle is an independent successor to
[Spider](https://github.com/Zaedus/spider). The complete Git history and the
original authorship of Zaedus are preserved. New and updated project code is
licensed under `GPL-3.0-or-later`; see [COPYING](COPYING).

## Roadmap

- v0.2 — runtime permissions, popup/OAuth windows, zoom, fullscreen,
  notifications, and improved downloads.
- v0.3 — Bastle backup/import, optional site-data transfer, aarch64, and KDE
  portal compatibility.
- v0.4 — privacy and power features after a dedicated threat model.

See [CONTRIBUTING.md](CONTRIBUTING.md) before proposing changes.
