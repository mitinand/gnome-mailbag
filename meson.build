# SPDX-License-Identifier: GPL-3.0-or-later
project('mailbag', meson_version: '>=1.3.0')

cargo = find_program('cargo')
custom_target(
  'mailbag',
  output: 'mailbag',
  command: [
    'sh', '-c',
    '"$1" build --locked --release --manifest-path "$2/Cargo.toml" --target-dir "$3/target" && cp "$3/target/release/mailbag" "$4"',
    'cargo-build', cargo, meson.project_source_root(), meson.project_build_root(), '@OUTPUT@',
  ],
  build_by_default: true,
  build_always_stale: true, # Cargo tracks Rust inputs and reuses unchanged outputs.
  install: true,
  install_dir: get_option('bindir'),
)

app_id = 'io.github.mitinand.Mailbag'
install_data('data' / app_id + '.desktop', install_dir: get_option('datadir') / 'applications')
install_data('data' / app_id + '.metainfo.xml', install_dir: get_option('datadir') / 'metainfo')
install_data('data' / app_id + '.svg', install_dir: get_option('datadir') / 'icons/hicolor/scalable/apps')
install_data('LICENSE', install_dir: get_option('datadir') / 'licenses' / app_id)
