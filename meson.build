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
install_data(
    'LICENSE', 'LICENSES/CC0-1.0.txt', 'LICENSES/CC-BY-SA-4.0.txt',
    'LICENSES/LGPL-2.0-or-later.txt',
    'crates/mailbag/resources/icons/account-icons.license',
    install_dir: license_dir,
)

# Install each vendored crate's notices and Cargo.toml under <name>-<version>/,
# keeping their paths inside the crate. Installable builds need vendored sources.
cargo_vendor_dir = get_option('cargo_vendor_dir')
if not cargo_vendor_dir.startswith('/') or not fs.is_dir(cargo_vendor_dir)
    error('Set cargo_vendor_dir to the absolute path of the vendored Cargo sources.')
endif
# stop-token publishes no license texts; ORIGIN.md explains the texts used instead.
notice_exceptions = {'stop-token': 'third-party-notices/stop-token'}
python = find_program('python3')
# One line per crate: vendor subdirectory, package name and version.
crates = run_command(
    python, '-c', '''
import pathlib, sys, tomllib
for crate in sorted(pathlib.Path(sys.argv[1]).iterdir()):
    if crate.is_dir():
        package = tomllib.loads((crate / "Cargo.toml").read_text(encoding="utf-8"))["package"]
        print(crate.name, package["name"], package["version"], sep="\t")
''',
    cargo_vendor_dir,
    check: true,
).stdout().strip().split('\n')
foreach crate : crates
    crate_fields = crate.split('\t')
    crate_dir = cargo_vendor_dir / crate_fields[0]
    crate_name = crate_fields[1]
    crate_license_dir = license_dir / (crate_name + '-' + crate_fields[2])
    # Named notice files anywhere in the crate, and the REUSE LICENSES directory.
    notices = run_command(
        'find', crate_dir, '-type', 'f', '(', '-iname', '*license*', '-o', '-iname', '*licence*',
        '-o', '-iname', 'copying*', '-o', '-iname', 'copyright*', '-o', '-iname', 'notice*',
        '-o', '-path', crate_dir / 'LICENSES' / '*', ')', '-printf', '%P\n',
        check: true,
    ).stdout().strip()
    if crate_name in notice_exceptions
        install_subdir(notice_exceptions[crate_name], install_dir: crate_license_dir, strip_directory: true)
    elif notices == ''
        error('Missing upstream license files in ' + crate_dir)
    endif
    foreach notice : notices == '' ? [] : notices.split('\n')
        notice_dir = fs.parent(notice)
        install_data(
            crate_dir / notice,
            install_dir: notice_dir == '.' ? crate_license_dir : crate_license_dir / notice_dir,
        )
    endforeach
    install_data(crate_dir / 'Cargo.toml', install_dir: crate_license_dir)
endforeach
