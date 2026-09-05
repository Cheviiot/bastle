<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Bastle Flatpak repository

Tagged releases publish a small OSTree repository to GitHub Pages without a
`gh-pages` branch. Both repository metadata and application commits are signed
by the dedicated Bastle release key:

```text
A697 39CC 6673 77C3 A671  BD85 885E 0C5C 26BD 31E1
```

The public key is tracked as `bastle-repository.gpg`. The private key is kept
only in the `BASTLE_FLATPAK_GPG_PRIVATE_KEY` GitHub Actions secret and the
owner's protected local release-key directory. Never commit or print it.

GitHub Pages receives an Actions deployment artifact, so publishing does not
create a persistent deployment branch. The repository contains the Bastle app
and its `io.github.cheviiot.bastle.Chromium` add-on for both supported
architectures. The add-on is mounted by Flatpak at
`/app/extensions/chromium`; it is not a second application.

GNOME Platform is intentionally not mirrored: `bastle.flatpakref` points
Flatpak to the upstream runtime repository, while Bastle and its Chromium
add-on come only from the project-owned remote.
