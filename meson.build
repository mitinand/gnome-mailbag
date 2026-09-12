# SPDX-License-Identifier: GPL-3.0-or-later
project('mailbag', meson_version: '>=1.3.0', default_options: ['buildtype=release'])

fs = import('fs')
cargo = find_program('cargo')
buildtype = get_option('buildtype')
if buildtype == 'debug'
    cargo_profile = 'dev'
    cargo_output = 'debug'
elif buildtype == 'release'
    cargo_profile = 'release'
    cargo_output = 'release'
else
    error('Supported build types are debug and release.')
endif
custom_target(
    'mailbag',
    output: 'mailbag',
    command: [
        'sh', '-c',
        '"$1" build --locked --package mailbag --profile "$5" --manifest-path "$2/Cargo.toml" --target-dir "$3/target" && cp "$3/target/$6/mailbag" "$4"',
        'cargo-build', cargo, meson.project_source_root(), meson.project_build_root(), '@OUTPUT@',
        cargo_profile, cargo_output,
    ],
    build_always_stale: true, # Cargo tracks Rust inputs and reuses unchanged outputs.
    install: true,
    install_dir: get_option('bindir'),
)

app_id = 'io.github.mitinand.Mailbag'
install_data('data' / app_id + '.desktop', install_dir: get_option('datadir') / 'applications')
install_data('data' / app_id + '.metainfo.xml', install_dir: get_option('datadir') / 'metainfo')
install_data('data' / app_id + '.svg', install_dir: get_option('datadir') / 'icons/hicolor/scalable/apps')
license_dir = get_option('datadir') / 'licenses' / app_id
install_data('LICENSE', 'LICENSES/CC0-1.0.txt', install_dir: license_dir)

# Preserve upstream notices, including their paths and package versions.
# Vendoring is required before configuring an installable build.
crates = run_command('find', 'vendor', '-mindepth', '1', '-maxdepth', '1', '-type', 'd', check: true).stdout().strip().split('\n')
foreach crate : crates
    notices = run_command(
        'find', crate, '-type', 'f', '(', '-iname', '*license*', '-o', '-iname', '*licence*',
        '-o', '-iname', 'copying*', '-o', '-iname', 'copyright*', '-o', '-iname', 'notice*', ')',
        check: true,
    ).stdout().strip()
    if notices == ''
        error('Missing upstream license files in ' + crate)
    endif
    foreach notice : notices.split('\n') + [crate / 'Cargo.toml']
        install_data(notice, install_dir: license_dir / fs.parent(notice))
    endforeach
endforeach
