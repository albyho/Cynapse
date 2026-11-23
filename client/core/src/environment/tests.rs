use super::*;

#[test]
fn compares_versions_found_in_tool_output() {
    assert!(output_satisfies_minimum_version(
        "rustc 1.87.0 (17067e9ac 2025-05-09)",
        "1.85.0"
    ));
    assert!(output_satisfies_minimum_version(
        "cargo 1.85.0 (d73d2caf9 2024-12-31)",
        "1.85.0"
    ));
    assert!(!output_satisfies_minimum_version(
        "rustc 1.84.9 (abc 2024-01-01)",
        "1.85.0"
    ));
    assert!(!output_satisfies_minimum_version(
        "command not found",
        "1.85.0"
    ));
}
