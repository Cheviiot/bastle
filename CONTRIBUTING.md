# Contributing to Bastle

Thank you for helping build Bastle. Please search existing issues before filing
a report and keep each pull request focused on one change.

## Workflow

1. Create a branch from `main`.
2. Work inside the documented `bastle-dev` Fedora 44 Distrobox.
3. Add tests for behavior changes.
4. Run the local gate:

   ```sh
   cargo fmt --check
   cargo check
   cargo clippy --all-targets -- -D warnings
   cargo test
   meson setup build
   meson compile -C build
   meson test -C build
   ```

5. Open a pull request. Changes are squash-merged after required checks pass.

## Development environment

Project-specific packages belong in the Fedora 44 Distrobox named
`bastle-dev`, not on the ALT Workstation host:

```sh
distrobox create --name bastle-dev --image registry.fedoraproject.org/fedora:44 --yes
distrobox enter bastle-dev -- sudo dnf install -y rust cargo rustfmt clippy cargo-deny gcc pkgconf-pkg-config meson ninja-build blueprint-compiler gtk4-devel libadwaita-devel webkitgtk6.0-devel openssl-devel appstream desktop-file-utils flatpak-builder gettext glib2-devel librsvg2-tools xorg-x11-server-Xvfb dbus-daemon git nodejs python3-aiohttp python3-pyyaml python3-tomlkit
```

Build the development Flatpak without FUSE-backed rofiles:

```sh
distrobox enter bastle-dev -- flatpak-builder --disable-rofiles-fuse --user --install --force-clean --install-deps-from=flathub .flatpak-build build-aux/io.github.cheviiot.bastle.Devel.json
```

Run every GUI check on a virtual X11 display. Never allow it to fall back to
the active Wayland session:

```sh
distrobox enter bastle-dev -- dbus-run-session -- env -u WAYLAND_DISPLAY xvfb-run -a timeout 10s flatpak run --nosocket=wayland --socket=x11 --env=GDK_BACKEND=x11 io.github.cheviiot.bastle
```

Use English for code, identifiers, technical documentation, and commit
messages. User-facing strings must be translatable; update the Russian catalog
when adding UI text.

Do not add broad Flatpak permissions, another browser engine or runtime, WebKit
patches, Chromium user-agent shims, or access to another application's sandbox.
Discuss any permission change in an issue first.

By contributing, you agree that your contribution is provided under
`GPL-3.0-only`.
