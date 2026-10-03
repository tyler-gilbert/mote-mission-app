"""
Load the spaces starlark SDK and packages repositories.
"""

load("//@star/prelude/rules/checkout.star", "checkout_add_env_vars")
load("//@star/prelude/rules/env.star", "env_append")
load("//@star/prelude/rules/sdk.star", "sdk_add_repo", "sdk_is_owner")

sdk_add_repo(
    "@star/packages",
    url = "https://github.com/work-spaces/packages",
    rev = "add-package-tracking",
)

if sdk_is_owner():
    # Ensure tools checked out to sysroot/bin are available
    # during checkout_add_exec() calls
    checkout_add_env_vars(
        "sysroot_env_path",
        vars = [
            env_append("PATH", "{}/sysroot/bin".format(workspace.get_absolute_path()), help = "Add sysroot/bin to the PATH"),
        ],
    )
