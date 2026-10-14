"""
Configure the SDK.
"""

load("//@star/packages/star/rust.star", "rust_add")
load("//@star/packages/star/spaces-cli.star", "spaces_add_devutils", "spaces_add_star_formatter")
load("//@star/packages/star/starship.star", "starship_add_bash")
load("//@star/prelude/info.star", "info_is_ci")
load(
    "//@star/prelude/rules/checkout.star",
    "checkout_add_archive",
    "checkout_add_env_vars",
    "checkout_add_repo",
    "checkout_update_asset",
)
load("//@star/prelude/rules/sdk.star", "sdk_is_owner")
load("//@star/prelude/rules/ws.star", "workspace_load_value")

if sdk_is_owner():
    spaces_add_devutils(
        "spaces0",
        "v0.22.2",
        devutils_version = "devutils-v0.1.16",
        system_paths = ["/usr/bin", "/bin"],
        is_activate_sccache = not info_is_ci(),
    )

    spaces_add_star_formatter(
        "star_formatter0",
        configure_zed = True,
        deps = [":spaces0"],
    )

    starship_add_bash(
        "starship0",
        shortcuts = {},
        install_binary = False,
        deps = [":spaces0"],
    )

    rust_add(
        "rust_toolchain",
        version = "1.95",
        deps = [":spaces0"],
    )

_MOTE_FIRMWARE_REV = workspace_load_value("MOTE_FIRMWARE_REV") or "spacify-mote-bringup"

checkout_add_repo(
    "mote",
    url = "https://github.com/tyler-gilbert/mote",
    rev = _MOTE_FIRMWARE_REV,
)

checkout_add_repo(
    "bee-ui",
    url = "https://github.com/work-spaces/bee-ui",
    rev = "main",
)

checkout_update_asset(
    "vscode_rust_analyzer_extension",
    destination = "//.vscode/extensions.json",
    format = "json",
    value = {
        "recommendations": ["rust-lang.rust-analyzer"],
    },
)

checkout_update_asset(
    "vscode_linked_projects",
    destination = "//.vscode/settings.json",
    format = "json",
    value = {
        "rust-analyzer.linkedProjects": [
            "mote-mission-app/crates/app/Cargo.toml",
            "mote/mote-firmware/Cargo.toml",
        ],
    },
)

checkout_update_asset(
    "zed_linked_projects",
    destination = "//.zed/settings.json",
    format = "json",
    value = {
        "lsp": {
            "rust-analyzer": {
                "initialization_options": {
                    "linkedProjects": [
                        "mote-mission-app/crates/app/Cargo.toml",
                        "mote/mote-firmware/Cargo.toml",
                    ],
                },
            },
        },
    },
)

checkout_add_archive(
    "telemetry_test_data",
    url = "https://github.com/tyler-gilbert/mote-mission-app/releases/download/v0.1.0/telemetry-test-data.zip",
    sha256 = "4d22e5dd85bb3c9ac990421758960f04e6adbdfb7537b199f45d9cfe31e55022",
    add_prefix = "telemetry-test-data",
)
