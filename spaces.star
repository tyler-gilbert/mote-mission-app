"""
Spaces rules for developing the native Mote application.
"""

load("//@star/prelude/rules/deps.star", "deps_new")
load("//@star/prelude/rules/run.star", "run_add", "run_add_exec", "run_log_level_passthrough")

_DEPS = deps_new(rules = ["//bee-ui:cargo_fonts_icons"], files = ["src/**/*.rs"])

run_add_exec(
    "check",
    command = "cargo",
    args = ["check"],
    working_directory = ".",
    deps = _DEPS,
)

run_add_exec(
    "build",
    command = "cargo",
    args = ["build"],
    working_directory = ".",
    deps = _DEPS,
)

run_add_exec(
    "run",
    command = "cargo",
    args = ["run"],
    working_directory = ".",
    deps = _DEPS,
)

run_add_exec(
    "test",
    command = "cargo",
    args = ["test"],
    working_directory = ".",
    deps = _DEPS,
)

run_add_exec(
    "format",
    command = "cargo",
    args = ["fmt"],
    working_directory = ".",
    deps = _DEPS,
)

run_add_exec(
    "format_check",
    command = "cargo",
    args = ["fmt", "--check"],
    working_directory = ".",
    deps = _DEPS,
)

run_add_exec(
    "lint",
    command = "cargo",
    args = ["clippy", "--all-targets", "--all-features", "--", "-D", "warnings"],
    working_directory = ".",
    deps = _DEPS,
)

run_add(
    "ci",
    deps = [":check", ":build", ":format_check", ":lint", ":test"],
)
