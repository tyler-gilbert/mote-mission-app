"""
Spaces rules for developing the native Mote application.
"""

load("//@star/prelude/rules/deps.star", "deps_new")
load("//@star/prelude/rules/rules.star", "rules_as_rule", "rules_new")
load("//@star/prelude/rules/run.star", "run_add", "run_add_exec", "run_log_level_passthrough")

def cargo_deps_new(name: str, deps = [], files = [], has_fixtures = False):
    fixtures = ["crates/{}/fixtures/**".format(name)] if has_fixtures else []
    return deps_new(rules = deps, files = [
        "crates/{}/src/**".format(name),
        "**/Cargo.toml",
        "//mote/mote-api/**",
    ] + fixtures + files)

def add_cargo_project(name: str, deps = [], has_fixtures = False):
    working_directory = "./crates/" + name
    rule_names = ["check", "build", "run", "test", "format", "format_check", "lint", "ci"]
    rules = rules_new(name, rule_names)
    cargo_deps = cargo_deps_new(name, deps, has_fixtures = has_fixtures)
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
        deps = cargo_deps,
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

add_cargo_project(
    "app",
    deps = ["//bee-ui:cargo_fonts_icons"],
)

add_cargo_project("sim", has_fixtures = True)

sim_working_directory = "./crates/sim"

run_add_exec(
    "sim_navigation",
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
    working_directory = sim_working_directory,
    deps = cargo_deps_new("sim", has_fixtures = True),
    help = "Replay the navigation sensor fixtures through the sim crate",
)

run_add_exec(
    "sim_guidance",
    command = "cargo",
    args = [
        "run",
        "--",
        "guidance",
        "../../../san_martin_pattern.json",
        "fixtures/san_martin_guidance.json",
    ],
    working_directory = sim_working_directory,
    deps = cargo_deps_new("sim", files = ["//san_martin_pattern.json"], has_fixtures = True),
    help = "Run the guidance test vector against the San Martin route through the sim crate",
)

run_add_exec(
    "sim_control",
    command = "cargo",
    args = [
        "run",
        "--",
        "control",
        "../../../san_martin_pattern.json",
        "fixtures/control_vector.json",
    ],
    working_directory = sim_working_directory,
    deps = cargo_deps_new("sim", files = ["//san_martin_pattern.json"], has_fixtures = True),
    help = "Run the control test vector in the San Martin reference frame through the sim crate",
)
