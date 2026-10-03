"""
Configure the SDK.
"""

load("//@star/packages/star/rust.star", "rust_add")
load("//@star/packages/star/spaces-cli.star", "spaces_add_devutils", "spaces_add_star_formatter")
load("//@star/packages/star/starship.star", "starship_add_bash")
load("//@star/packagesstar/config.star", "CONFIG_TRACKER_OPTION")
load("//@star/prelude/info.star", "info_is_ci")
load("//@star/prelude/rules/checkout.star", "checkout_add_env_vars", "checkout_add_repo")
load("//@star/prelude/rules/sdk.star", "sdk_is_owner")
load("//@star/sdk/star/checkout-config.star", "checkout_config_store_option")

checkout_config_store_option(CONFIG_TRACKER_OPTION, True)

rust_add(
    "rust_toolchain",
    version = "1.95",
    deps = [":rust_workspace_toolchain"],
)

spaces_add_devutils(
    "spaces0",
    "v0.21.0",
    devutils_version = "devutils-v0.1.15",
    system_paths = ["/usr/bin", "/bin"],
    is_activate_sccache = not info_is_ci(),
)

spaces_add_star_formatter(
    "star_formatter0",
    configure_zed = True,
)

starship_add_bash(
    "starship0",
    shortcuts = {},
    install_binary = False,
    deps = [":spaces0"],
)

checkout_add_repo(
    "mote",
    url = "https://github.com/tyler-gilbert/mote",
    rev = "spacify-mote-bringup",
)

checkout_add_repo(
    "bee-ui",
    url = "https://github.com/work-spaces/bee-ui",
    rev = "main",
)
