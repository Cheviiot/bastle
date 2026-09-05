<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Bastle built-in Chromium engine protocol v1

The Chromium engine is an internal process included in the same Bastle Flatpak.
Bastle talks to session-bus name `io.github.cheviiot.bastle.Chromium`, object
`/io/github/cheviiot/bastle/Chromium/Engine1`, interface
`io.github.cheviiot.bastle.Chromium.Engine1`. The manifest permits Bastle to own
only that sub-name; no second Flatpak identity or package is involved.

## Authentication and profile ownership

The D-Bus boundary separates Bastle's manager process from the engine broker;
it is not a separate sandbox trust boundary. A 256-bit random capability token
is generated for each app ID. The engine binds an app ID to the first valid
token it sees and requires an exact, constant-time token match for later calls.
Tokens are never included in `.bastle-backup` archives. The host user remains
inside the trust boundary, as with other per-user D-Bus services.

Each app ID owns a separate Electron `userData` directory and persistent
partition. Switching engines does not copy or remove either engine's profile.
Profile deletion failures are retained by Bastle and retried when the built-in
engine service next becomes available.

## Methods

```xml
<node>
  <interface name="io.github.cheviiot.bastle.Chromium.Engine1">
    <method name="GetCapabilities">
      <arg name="protocol_version" type="u" direction="out"/>
      <arg name="features" type="as" direction="out"/>
    </method>
    <method name="OpenApp">
      <arg name="id" type="s" direction="in"/>
      <arg name="url" type="s" direction="in"/>
      <arg name="title" type="s" direction="in"/>
      <arg name="user_agent" type="s" direction="in"/>
      <arg name="width" type="i" direction="in"/>
      <arg name="height" type="i" direction="in"/>
      <arg name="maximized" type="b" direction="in"/>
      <arg name="start_in_background" type="b" direction="in"/>
      <arg name="token" type="s" direction="in"/>
      <arg name="policy_json" type="s" direction="in"/>
    </method>
    <method name="DeleteProfile">
      <arg name="id" type="s" direction="in"/>
      <arg name="token" type="s" direction="in"/>
    </method>
  </interface>
</node>
```

Protocol v1 features are `open-app`, `policy-v2`, `profile-delete`,
`permissions`, `navigation-allowlist`, `proxy`, `background`,
`download-dialog`, and `oauth-popups`. Bastle rejects a different protocol
version and does not silently fall back to WebKitGTK.

`policy_json` is a validated `AppPolicyV2` document with WebKit-only content
filter lists removed. Unknown fields and permission kinds are denied by the
engine rather than granted. An engine error is shown to the user with an
explicit one-time WebKitGTK fallback.
