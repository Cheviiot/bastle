# SPDX-License-Identifier: GPL-3.0-or-later

pkgdatadir = get_option('prefix') / get_option('datadir') / meson.project_name()
gnome = import('gnome')

blueprints = custom_target('blueprints',
  input: files(
    'gtk/help-overlay.blp',
    'window.blp',
    'create_app_dialog.blp',
    'app_row.blp',
    'app_window.blp',
    'app_page.blp',
    'home_page.blp',
  ),
  output: '.',
  command: [find_program('blueprint-compiler'), 'batch-compile', '@OUTPUT@', '@CURRENT_SOURCE_DIR@', '@INPUT@'],
)

gnome.compile_resources('bastle',
  'bastle.gresource.xml',
  gresource_bundle: true,
  install: true,
  install_dir: pkgdatadir,
  dependencies: blueprints,
)

cargo_bin  = find_program('cargo')
cargo_env  = [
  'BASTLE_LOCALEDIR=' + get_option('prefix') / get_option('localedir'),
  'BASTLE_PKGDATADIR=' + pkgdatadir,
]

if get_option('flatpak')
  cargo_offline = 'true'
  buildtype  = 'release'
else
  cargo_env += [ 'CARGO_HOME=' + meson.project_build_root()  / 'cargo-home' ]
  cargo_offline = 'false'
  buildtype  = get_option('buildtype')
endif

if buildtype == 'release'
  rust_target = 'release'
else
  rust_target = 'debug'
endif


cargo_build = custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: meson.project_name(),
  console: true,
  install: true,
  install_dir: get_option('bindir'),
  command: [
    'env', cargo_env,
    find_program('sh'), meson.project_source_root() / 'build-aux/cargo-build.sh',
    cargo_bin, meson.project_source_root(), meson.project_build_root() / 'src',
    rust_target, cargo_offline, '@OUTPUT@',
  ]
)
