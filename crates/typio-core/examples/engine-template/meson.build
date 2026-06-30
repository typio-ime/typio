project('typio-engine-hello', 'c',
    version: '0.1.1',
    default_options: [
        'c_std=c23',
        'warning_level=2',
        'werror=false',
    ],
    meson_version: '>=1.0.0',
)

cc = meson.get_compiler('c')

add_project_arguments('-D_POSIX_C_SOURCE=200809L', language: 'c')

# ── Dependencies ──────────────────────────────────────────────────────────
# Prefer the system-installed ABI; fall back to a `subprojects/libtypio`
# checkout so the template can build against an in-tree libtypio for local
# verification (meson setup --force-fallback-for=libtypio).
typio_abi_dep = dependency('typio-engine-abi',
    fallback: ['libtypio', 'libtypio_engine_abi_dep'])

# ── Install paths ─────────────────────────────────────────────────────────
# Typio hosts spawn the engine worker named by the manifest's `command`
# and discover engines by scanning <datadir>/typio/engines for
# typio-engine-*.toml manifests.
worker_install_dir = get_option('libexecdir') / 'typio' / 'engines'
manifest_install_dir = get_option('datadir') / 'typio' / 'engines'

# ── Icon assets ───────────────────────────────────────────────────────────
# The manifest's `icon` key names a freedesktop icon; install the bundled
# SVG into the hicolor theme so hosts can resolve it.
icon_dir = get_option('datadir') / 'icons' / 'hicolor' / 'symbolic' / 'apps'
install_data('data/icons/hicolor/symbolic/apps/typio-engine-hello-symbolic.svg',
    install_dir: icon_dir,
)

# ── Build ─────────────────────────────────────────────────────────────────
# Engine-process worker: hello_engine.c keeps the classic ABI vtable, and
# worker_main.c wraps it in the fd-3 Typio Engine Protocol.
hello_worker = executable('typio-engine-hello',
    files('src/hello_engine.c', 'src/worker_main.c'),
    dependencies: [typio_abi_dep],
    install: true,
    install_dir: worker_install_dir,
)

# Development manifest: points at the worker in the build directory so a
# host can be pointed straight at it (typioctl engine load, engine_dirs).
dev_manifest_conf = configuration_data()
dev_manifest_conf.set('ENGINE_COMMAND', './typio-engine-hello')
configure_file(
    input: 'typio-engine-hello.toml.in',
    output: 'typio-engine-hello.toml',
    configuration: dev_manifest_conf,
)

# Installed manifest: absolute path into libexecdir.
install_manifest_conf = configuration_data()
install_manifest_conf.set(
    'ENGINE_COMMAND',
    get_option('prefix') / worker_install_dir / 'typio-engine-hello',
)
installed_manifest = configure_file(
    input: 'typio-engine-hello.toml.in',
    output: 'typio-engine-hello.installed.toml',
    configuration: install_manifest_conf,
)
install_data(installed_manifest,
    install_dir: manifest_install_dir,
    rename: 'typio-engine-hello.toml',
)
