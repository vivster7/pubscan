//! Tests the interaction of the `analyze api` command.

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
    // Redact timestamps in log output
    (r"\[\d{4}-\d{2}-\d{2}\]\[\d{2}:\d{2}:\d{2}\]", "[TIMESTAMP]"),
];

// Create a Python package structure for testing
fn setup_test_package(root: &Path) -> Result<()> {
    // Create package structure
    fs::create_dir_all(root.join("mypackage"))?;

    // Create __init__.py
    fs::write(root.join("mypackage").join("__init__.py"), "")?;

    // Core module
    fs::write(
        root.join("mypackage").join("core.py"),
        indoc::indoc! {r#"
        """Core module with main functionality."""

        def add(a, b):
            """Add two numbers."""
            return a + b

        def subtract(a, b):
            """Subtract b from a."""
            return a - b

        def multiply(a, b):
            """Multiply two numbers."""
            return a * b

        # Private function should not be part of public API
        def _internal_helper():
            """This is an internal function."""
            return "Not meant to be used directly"
        "#},
    )?;

    // Utils module
    fs::write(
        root.join("mypackage").join("utils.py"),
        indoc::indoc! {r#"
        """Utility functions that build on core functionality."""

        from mypackage.core import add, multiply

        def add_multiple(*args):
            """Add multiple numbers."""
            result = 0
            for arg in args:
                result = add(result, arg)
            return result

        def multiply_multiple(*args):
            """Multiply multiple numbers."""
            if not args:
                return 0
            result = 1
            for arg in args:
                result = multiply(result, arg)
            return result
        "#},
    )?;

    // Models module
    fs::write(
        root.join("mypackage").join("models.py"),
        indoc::indoc! {r#"
        """Module containing data models."""

        class User:
            """User model representing a system user."""

            def __init__(self, name, email):
                self.name = name
                self.email = email

            def get_display_name(self):
                """Return a display-friendly name."""
                return f"{self.name} <{self.email}>"

        class _InternalModel:
            """Internal model not meant for external use."""
            pass
        "#},
    )?;

    // Config module
    fs::write(
        root.join("mypackage").join("config.py"),
        indoc::indoc! {r#"
        """Configuration utilities."""

        __all__ = ['DEFAULT_CONFIG', 'load_config', 'save_config']

        DEFAULT_CONFIG = {
            'debug': False,
            'timeout': 30,
            'max_retries': 3
        }

        def load_config(path):
            """Load configuration from a file."""
            pass

        def save_config(config, path):
            """Save configuration to a file."""
            pass

        def _validate_config(config):
            """Internal function to validate config."""
            pass
        "#},
    )?;

    // Client script outside package
    fs::write(
        root.join("client.py"),
        indoc::indoc! {r#"
        """Client code that uses the mypackage package."""

        # Direct imports for some symbols
        from mypackage.core import add, subtract
        from mypackage.utils import add_multiple
        from mypackage.models import User

        # Import the modules for module.symbol access pattern
        import mypackage.core
        import mypackage.config

        def main():
            """Initialize the application."""
            # Direct usage of imported symbols
            print(f"add(5, 3) = {add(5, 3)}")
            print(f"subtract(10, 4) = {subtract(10, 4)}")
            print(f"add_multiple(1, 2, 3, 4) = {add_multiple(1, 2, 3, 4)}")

            # Create and use a User object
            user = User("John", "john@example.com")
            print(f"User: {user.get_display_name()}")

            # Test the module.symbol access pattern
            print(f"multiply(2, 3) = {mypackage.core.multiply(2, 3)}")

            # Use config constants via module.symbol access
            config = mypackage.config.DEFAULT_CONFIG.copy()
            print(f"Default timeout: {config['timeout']}")

            # Use config functions via module.symbol access
            mypackage.config.save_config(config, 'config.json')
            loaded_config = mypackage.config.load_config('config.json')

            return config

        if __name__ == "__main__":
            main()
        "#},
    )?;

    Ok(())
}

/// Test analyzing a single module for its public API
#[test]
fn analyze_single_module() -> Result<()> {
    let tempdir = TempDir::new()?;
    let root = tempdir.path();

    setup_test_package(root)?;

    // Run the API analysis on the core module
    insta::with_settings!({
        filters => INSTA_FILTERS.to_vec(),
        snapshot_suffix => "text_format",
    }, {
        assert_cmd_snapshot!(
            command()
                .arg(root.join("mypackage").join("core.py"))
                .current_dir(root)
        );
    });

    Ok(())
}

/// Test analyzing a package directory for its public API
#[test]
fn analyze_package() -> Result<()> {
    let tempdir = TempDir::new()?;
    let root = tempdir.path();

    setup_test_package(root)?;

    // Run the API analysis on the package directory with text output
    insta::with_settings!({
        filters => INSTA_FILTERS.to_vec(),
        snapshot_suffix => "text_format",
    }, {
        assert_cmd_snapshot!(
            command()
                .arg(root.join("mypackage"))
                .current_dir(root)
        );
    });

    // Run the API analysis on the package directory with JSON output
    insta::with_settings!({
        filters => INSTA_FILTERS.to_vec(),
        snapshot_suffix => "json_format",
    }, {
        assert_cmd_snapshot!(
            command()
                .arg(root.join("mypackage"))
                .arg("--output-format=json")
                .current_dir(root)
        );
    });

    Ok(())
}

/// Test analyzing a module with class definitions
#[test]
fn analyze_classes() -> Result<()> {
    let tempdir = TempDir::new()?;
    let root = tempdir.path();

    setup_test_package(root)?;

    // Run the API analysis on the models module
    insta::with_settings!({
        filters => INSTA_FILTERS.to_vec(),
    }, {
        assert_cmd_snapshot!(
            command()
                .arg(root.join("mypackage").join("models.py"))
                .current_dir(root)
        );
    });

    Ok(())
}

/// Test analyzing a module with __all__ definitions
#[test]
fn analyze_all_definition() -> Result<()> {
    let tempdir = TempDir::new()?;
    let root = tempdir.path();

    setup_test_package(root)?;

    // Run the API analysis on the config module
    insta::with_settings!({
        filters => INSTA_FILTERS.to_vec(),
    }, {
        assert_cmd_snapshot!(
            command()
                .arg(root.join("mypackage").join("config.py"))
                .current_dir(root)
        );
    });

    Ok(())
}
