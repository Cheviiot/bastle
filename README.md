# Bastle

![Bastle icon](data/icons/hicolor/scalable/apps/io.github.cheviiot.bastle.svg)

Bastle is a GNOME-native manager for isolated web applications. WebKitGTK is
the default engine, and a Chromium engine is included in the same Bastle
Flatpak for sites that need it. Bastle creates desktop launchers through the
Dynamic Launcher Portal and keeps each application's metadata, profile,
cookies, and cache separate.

The name *bastle* means a detached fortified house: each website gets its own
small, self-contained space. The friendly shelter icon reflects that idea; the
project is not positioned as a security product.

## Status

The current release targets x86_64 and aarch64, GNOME 50, and
Flatpak. It supports application creation, editing, launching, deletion,
launcher repair, offline creation, popup/OAuth windows, navigation controls,
zoom, fullscreen, runtime permissions, managed downloads, and native
notifications. The v0.4 policy layer adds optional top-level origin
restrictions, per-app WebKit proxy selection, portal-authorized background
activity, and user-imported WebKit content filters. v0.5 adds an explicit
per-app engine choice and a local compatibility catalog; v0.5.1 embeds the
Chromium engine into the single Bastle package. Unknown sites continue to use
WebKitGTK, and Chromium is used only after explicit confirmation.

Flathub submission is intentionally deferred until Bastle has an independent
release history and enough differentiation from Spider.

## Data layout

```text
$XDG_DATA_HOME/bastle/apps/<id>/app.json
$XDG_DATA_HOME/bastle/apps/<id>/icon.png
$XDG_DATA_HOME/bastle/apps/<id>/policy.json
$XDG_DATA_HOME/bastle/profiles/<id>/
$XDG_CACHE_HOME/bastle/<id>/
```

GSettings stores only the main-window size. Application configuration,
permission decisions, and opt-in privacy/power settings live in versioned,
atomically written JSON files. Content-filter source rules are embedded in the
per-app policy so ordinary backups remain self-contained.

## Backup and restore

Bastle backups use the versioned `.bastle-backup` format: a deterministic
`tar.zst` archive containing application configuration, icons, and permission
policies. Cache and downloads are never exported. Cookies and WebKit site
storage are optional for WebKitGTK apps; selecting them requires passphrase
encryption with `age`, and the application must not be running while its
profile is copied. Chromium data stays in separate per-engine profile and cache
directories inside the Bastle sandbox and is never mistaken for WebKit data.
Chromium site-data export is not supported yet.

Restore shows a preview before making changes. Identical applications are
skipped, ID conflicts receive a fresh Bastle ID, and each launcher is installed
as its own portal-backed transaction. Archives containing absolute paths,
path traversal, links, duplicate entries, or unexpected files are rejected.

## Development environment

Security boundaries and known limitations are documented in the
[threat model](docs/threat-model.md). The internal engine process boundary is
specified in the [Chromium protocol](docs/chromium-engine-protocol.md).
Desktop integration requirements, detected interface versions, and isolated
GNOME/KDE smoke procedures are in the
[portal compatibility guide](docs/portal-compatibility.md).

Development is reproducible in the Fedora 44 Distrobox named `bastle-dev`:

```sh
distrobox create --name bastle-dev --image registry.fedoraproject.org/fedora:44 --yes
distrobox enter bastle-dev -- sudo dnf install -y rust cargo rustfmt clippy cargo-deny gcc pkgconf-pkg-config meson ninja-build blueprint-compiler gtk4-devel libadwaita-devel webkitgtk6.0-devel openssl-devel appstream desktop-file-utils flatpak-builder gettext glib2-devel librsvg2-tools xorg-x11-server-Xvfb dbus-daemon git nodejs python3-aiohttp python3-pyyaml python3-tomlkit
distrobox enter bastle-dev -- meson setup build
distrobox enter bastle-dev -- meson compile -C build
```

To build the Flatpak from inside Distrobox (where FUSE mounts are intentionally
restricted), use:

```sh
distrobox enter bastle-dev -- flatpak-builder --disable-rofiles-fuse --user --install --force-clean --install-deps-from=flathub .flatpak-build build-aux/io.github.cheviiot.bastle.Devel.json
```

GUI smoke tests must use a dedicated virtual display and explicitly disable
the Wayland socket so Flatpak cannot fall back to the active desktop session:

```sh
distrobox enter bastle-dev -- dbus-run-session -- env -u WAYLAND_DISPLAY xvfb-run -a timeout 10s flatpak run --nosocket=wayland --socket=x11 --env=GDK_BACKEND=x11 io.github.cheviiot.bastle
```

After changing `Cargo.lock`, regenerate the offline Flatpak sources with the
official generator pinned to the revision used for this development series:

```sh
distrobox enter bastle-dev
bastle_tools_dir=$(mktemp -d /tmp/bastle-flatpak-tools.XXXXXX)
git clone https://github.com/flatpak/flatpak-builder-tools.git "$bastle_tools_dir"
git -C "$bastle_tools_dir" checkout 1fc32195e3e60fe5c97f0af646dec7a99df5962b
python3 "$bastle_tools_dir/cargo/flatpak-cargo-generator.py" Cargo.lock -o build-aux/generated-sources.json
rm -rf -- "$bastle_tools_dir"
```

Project-specific system packages are not installed on the ALT Workstation
host. The Flatpak manifest uses GNOME runtime 50, the compatible Electron 2
BaseApp 25.08, and the Rust SDK extension. Chromium source lives in
[`chromium/`](chromium/) and is built into `io.github.cheviiot.bastle`; it does
not create another application ID, Flatpak ref, or bundle.

## Desktop portal support

| Desktop stack | Dynamic Launcher | Backup/restore chooser | Status |
| --- | --- | --- | --- |
| GNOME 50 / `xdg-desktop-portal-gnome` 50.0 | Application and Webapp advertised | File Chooser plus Documents | Source-compatible; confirm the active session with **System Capabilities** |
| KDE Plasma 6.7 / `xdg-desktop-portal-kde` 6.7.4 | Application and Webapp advertised | File Chooser plus Documents | Best-effort; confirm the selected backend with **System Capabilities** |

The versions above were the current stable upstream releases checked on
2026-09-05. The active `portals.conf` can select a different backend, so the
installed package name alone is not proof of compatibility. Bastle probes the
live session and reports Dynamic Launcher, File Chooser, Documents, supported
launcher types, and their interface versions independently.

If application creation or repair fails, open **Menu → System Capabilities**.
`Unavailable` means that the active session does not expose the interface;
`Unsupported` means Dynamic Launcher exists but does not advertise the
Application launcher type. A cancelled confirmation is harmless, while a
denial is reported separately and preserves existing local data. Install or
select the correct portal backend for the desktop, restart the user session,
and retry. Bastle never works around a portal failure by writing directly to
the host launcher directory.

## Provenance and license

Bastle is an independent successor to
[Spider](https://github.com/Zaedus/spider). The complete Git history and the
original authorship of Zaedus are preserved. New and updated project code is
licensed under `GPL-3.0-or-later`; see [COPYING](COPYING).

Bastle does not read Spider settings or profiles and has no Spider import or
runtime compatibility path. Existing Bastle applications continue to use
their own IDs, metadata, launchers, and WebKit profiles.

## Roadmap

- v0.2 — runtime permissions, popup/OAuth windows, zoom, fullscreen,
  notifications, and managed downloads.
- v0.3 — Bastle backup/restore, optional encrypted site-data transfer, and
  aarch64 bundles.
- v0.4 — opt-in origin allowlists, per-app proxy settings, background/autostart
  through the desktop portal, content filters, and a dedicated threat model.
- v0.5 — an optional-per-app Chromium engine for sites that cannot run
  correctly on WebKitGTK, built into the same Flatpak since v0.5.1, plus live
  GNOME/KDE portal diagnostics.

See [CONTRIBUTING.md](CONTRIBUTING.md) before proposing changes.
