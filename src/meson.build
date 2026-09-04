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

resources = gnome.compile_resources('bastle',
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
cargo_features = ''
if get_option('ui_tests')
  cargo_features = 'ui-tests'
endif

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
    rust_target, cargo_offline, cargo_features, '@OUTPUT@',
  ]
)

if get_option('ui_tests')
  xvfb_run = find_program('xvfb-run')
  dbus_run_session = find_program('dbus-run-session')
  ui_test_env = environment()
  ui_test_env.set('WAYLAND_DISPLAY', '')
  ui_test_env.set('GDK_BACKEND', 'x11')
  ui_test_env.set('BASTLE_TEST_RESOURCE', resources.full_path())
  ui_test_env.set('GSETTINGS_SCHEMA_DIR', meson.project_build_root() / 'data')
  test(
    'UI smoke',
    dbus_run_session,
    args: ['--', xvfb_run.full_path(), '-a', cargo_build.full_path(), '--ui-test-app-page'],
    env: ui_test_env,
    depends: [cargo_build, resources, test_schema],
    timeout: 30,
  )
endif
