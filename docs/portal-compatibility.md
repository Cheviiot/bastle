<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Desktop portal compatibility

Bastle uses desktop portals for every host integration boundary. It never
writes `.desktop` launchers directly to the host when a portal is missing,
unsupported, cancelled, or denied.

## Required interfaces

| Operation | Required interface | Upstream interface version checked for v0.5 |
| --- | --- | --- |
| Create, repair, or remove a launcher | [`org.freedesktop.portal.DynamicLauncher`](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.DynamicLauncher.html) with the `Application` launcher type | 1 |
| Choose a backup, icon, filter list, download, or backup destination | [`org.freedesktop.portal.FileChooser`](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.FileChooser.html) | 4 |
| Make a selected backup available inside Flatpak | [`org.freedesktop.portal.Documents`](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Documents.html) | 5 |
| Optional background/autostart | `org.freedesktop.portal.Background` | Reported at runtime |

The System Capabilities dialog probes each interface independently, records
the version exposed by the active session, and displays the Dynamic Launcher
type bitmask as `Application` and `Web application` support. An unavailable
File Chooser does not hide a working Dynamic Launcher, and vice versa.

Portal operation failures have distinct meanings:

- **Unavailable** — the D-Bus interface or selected backend is absent.
- **Unsupported** — the interface exists but lacks the required launcher type
  or version.
- **Cancelled** — the user dismissed the request; no failure recovery is
  required.
- **Denied** — desktop policy or the user rejected the operation.
- **Failed** — the portal returned another operational error.

Create and restore remain staged transactions. A failed launcher installation
is rolled back; a failed launcher removal preserves local metadata and profile
data, except that an already missing launcher is accepted. No diagnostic path
grants direct host filesystem access.

## Upstream compatibility snapshot

This source-level snapshot was recorded on 2026-09-05. Runtime behavior still
depends on the distribution's `xdg-desktop-portal`, backend selection, desktop
policy, and document-portal mount.

| Stack | Current stable source reviewed | Relevant support in that source |
| --- | --- | --- |
| Portal frontend/document store | [`xdg-desktop-portal` 1.22.1](https://github.com/flatpak/xdg-desktop-portal/releases/tag/1.22.1) | Dynamic Launcher v1, File Chooser v4, Documents v5 |
| GNOME | [`xdg-desktop-portal-gnome` 50.0](https://gitlab.gnome.org/GNOME/xdg-desktop-portal-gnome/-/tags/50.0) | Dynamic Launcher advertises Application and Webapp; File Chooser implemented |
| KDE Plasma | [`xdg-desktop-portal-kde` 6.7.4](https://invent.kde.org/plasma/xdg-desktop-portal-kde/-/tags/v6.7.4) | Dynamic Launcher v1 advertises Application and Webapp; File Chooser implemented |

GNOME is the supported target. KDE remains best-effort and receives actionable
capability diagnostics rather than a promise of native integration.

## Isolated smoke procedure

Never run automated GUI checks against the active display. The repository's UI
test uses Xvfb with Wayland explicitly removed:

```sh
distrobox enter bastle-dev -- bash -lc \
  'cd /home/cheviiot/Project/Progs/Spider && \
   env -u WAYLAND_DISPLAY GDK_BACKEND=x11 \
   meson test -C build --print-errorlogs'
```

The Meson `UI smoke` test itself wraps the test-only binary in `xvfb-run -a`
and `dbus-run-session`; do not run that binary separately on the active display.

Run the following manual matrix only in a disposable GNOME Wayland or KDE
Plasma test session, never on the developer's active display:

1. Record desktop and portal package versions.
2. Open **System Capabilities** and record all reported interface versions and
   launcher types.
3. Create an offline application, approve its launcher, repair it, and remove
   it.
4. Cancel one launcher confirmation and verify that no application data is
   committed.
5. Deny one launcher request by desktop policy and verify that the diagnostic
   says `Denied`, not `Cancelled`.
6. Create a backup, cancel restore once, then restore it and verify that the
   File Chooser and Documents paths work.
7. Confirm that no launcher appears in the host filesystem except through the
   portal-managed location.

### Recorded results

| Environment | Result | Notes |
| --- | --- | --- |
| Xvfb + private D-Bus | Automated UI and fake-backend coverage | Required for every code change; no access to the active display |
| GNOME Wayland 50 | Pending owner-run disposable-session check | Not executed from this development environment because GUI access is restricted to a virtual display |
| KDE Plasma 6.7 | Pending owner-run disposable-session check | Source compatibility reviewed; no native KDE integration claim |

The pending real-session rows are deliberately explicit: an Xvfb run cannot
truthfully stand in for a selected GNOME or KDE portal backend.
