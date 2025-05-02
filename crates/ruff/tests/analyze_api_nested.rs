//! Tests the detection of module.symbol patterns in the `analyze api` command.

#![cfg(not(target_arch = "wasm32"))]
#![cfg(not(windows))]

use std::fs;
use std::path::Path;
use std::process::Command;
use std::str;

use anyhow::Result;
use insta_cmd::{assert_cmd_snapshot, get_cargo_bin};
use tempfile::TempDir;

fn command() -> Command {
    let mut command = Command::new(get_cargo_bin("ruff"));
    command.arg("analyze");
    command.arg("api");
    command
}

const INSTA_FILTERS: &[(&str, &str)] = &[
    // Rewrite Windows output to Unix output
    (r"\\", "/"),
    // Redact temporary paths
    (r"/tmp/\.tmp[^/]+", "[TEMPDIR]"),
];

// Create a test package structure with nested modules
fn setup_test_package(root: &Path) -> Result<()> {
    // Create package structure
    fs::create_dir_all(root.join("src").join("pkg1").join("pkg2"))?;

    // Create __init__.py files
    fs::write(root.join("src").join("__init__.py"), "")?;
    fs::write(root.join("src").join("pkg1").join("__init__.py"), "")?;

    // Create the nested module
    fs::write(
        root.join("src")
            .join("pkg1")
            .join("pkg2")
            .join("__init__.py"),
        indoc::indoc! {r#"
        from __future__ import annotations


        def some_function():
            """This is a sample function"""
            return "Hello from pkg2 function"


        class SampleClass:
            """This is a sample class"""

            def method(self):
                return "Hello from pkg2 class method"


        SOME_CONSTANT = "This is a constant"
        "#},
    )?;

    // Create client directory
    fs::create_dir_all(root.join("client"))?;

    // Create client that uses the module.symbol pattern
    fs::write(
        root.join("client").join("__init__.py"),
        indoc::indoc! {r#"
        # Import the module
        from __future__ import annotations

        from src.pkg1 import pkg2

        # Use function via module.symbol pattern
        result = pkg2.some_function()
        print(result)

        # Use class via module.symbol pattern
        instance = pkg2.SampleClass()
        print(instance.method())

        # Use constant via module.symbol pattern
        print(pkg2.SOME_CONSTANT)

        # Also test an if statement that uses the module
        if pkg2.SOME_CONSTANT == "test":
            print("Test logic")
        "#},
    )?;

    Ok(())
}

/// Test detecting module.symbol access patterns
#[test]
fn analyze_module_symbol_access() -> Result<()> {
    let tempdir = TempDir::new()?;
    let root = tempdir.path();

    setup_test_package(root)?;

    // Run the API analysis on the nested module
    insta::with_settings!({
        filters => INSTA_FILTERS.to_vec(),
    }, {
        assert_cmd_snapshot!(
            command()
                .arg(root.join("src").join("pkg1").join("pkg2").join("__init__.py"))
                .current_dir(root)
        );
    });

    Ok(())
}
