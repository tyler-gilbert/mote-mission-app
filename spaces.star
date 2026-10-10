"""
Spaces rules for developing the native Mote application.
"""

load("//@star/prelude/rules/deps.star", "deps_new")
load("//@star/prelude/rules/rules.star", "rules_as_rule", "rules_new")
load("//@star/prelude/rules/run.star", "run_add", "run_add_exec", "run_log_level_passthrough")

def add_cargo_project(name: str, deps = [], extra_rules = []):
    working_directory = "./crates/" + name
    rule_names = ["check", "build", "run", "test", "format", "format_check", "lint", "ci"] + extra_rules
    rules = rules_new(name, rule_names)
    cargo_deps = deps_new(rules = deps, files = [
        "crates/{}/src/**".format(name),
        "**/Cargo.toml",
        "//mote/mote-api/**",
    ])
    run_add_exec(
        rules_as_rule(rules, "check"),
        command = "cargo",
        args = ["check"],
        working_directory = working_directory,
        deps = cargo_deps,
        help = "Run cargo check for the {} crate".format(name),
    )

    run_add_exec(
        rules_as_rule(rules, "build"),
        command = "cargo",
        args = ["build"],
        working_directory = working_directory,
        deps = cargo_deps,
    )

    run_add_exec(
        rules_as_rule(rules, "run"),
        command = "cargo",
        args = ["run"],
        working_directory = working_directory,
        # No deps, always run
    )

    run_add_exec(
        rules_as_rule(rules, "test"),
        command = "cargo",
        args = ["test"],
        working_directory = working_directory,
        deps = cargo_deps,
    )

    run_add_exec(
        rules_as_rule(rules, "format"),
        command = "cargo",
        args = ["fmt"],
        working_directory = working_directory,
        deps = cargo_deps,
    )

    run_add_exec(
        rules_as_rule(rules, "format_check"),
        command = "cargo",
        args = ["fmt", "--check"],
        working_directory = working_directory,
        deps = cargo_deps,
    )

    run_add_exec(
        rules_as_rule(rules, "lint"),
        command = "cargo",
        args = ["clippy", "--all-targets", "--all-features", "--", "-D", "warnings"],
        working_directory = working_directory,
        deps = cargo_deps,
    )

    run_add(
        rules_as_rule(rules, "ci"),
        deps = [
            rules_as_rule(rules, "check"),
            rules_as_rule(rules, "build"),
            rules_as_rule(rules, "format_check"),
            rules_as_rule(rules, "lint"),
            rules_as_rule(rules, "test"),
        ],
    )

    if "navigation" in extra_rules:
        run_add_exec(
            rules_as_rule(rules, "navigation"),
            command = "cargo",
            args = [
                "run",
                "--",
                "navigation",
                "../../../telemetry_one_point_two_three_1791430559418.data",
                "../../../telemetry_one_point_two_three_1791430559418.json",
                "../../../telemetry_one_point_two_three_rotate90_1791430681872.data",
                "../../../telemetry_one_point_two_three_rotate90_1791430681872.json",
            ],
            working_directory = working_directory,
            deps = cargo_deps,
            help = "Replay the navigation sensor fixtures through the sim crate",
        )

add_cargo_project(
    "app",
    deps = ["//bee-ui:cargo_fonts_icons"],
)

add_cargo_project(
    "sim",
    extra_rules = ["navigation"],
)
