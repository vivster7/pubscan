"""Command-line interface for pubscan."""

from __future__ import annotations

import argparse
import os
import platform
import subprocess
import sys
from pathlib import Path
from typing import List, Optional

# Import version from package
try:
    from pubscan import __version__
except ImportError:
    __version__ = "0.1.0"  # Fallback version


def find_binary() -> Optional[Path]:
    """Find the api-analyzer binary."""
    # Look in common locations
    package_dir = Path(__file__).parent
    workspace_root = Path().absolute()

    # Try to find the workspace root by looking for the Cargo.toml
    current = Path().absolute()
    while current != current.parent:
        if (current / "Cargo.toml").exists():
            workspace_root = current
            break
        current = current.parent

    # Check if we're in development mode or installed
    possible_locations = [
        # Development locations - navigate to target in workspace root
        workspace_root / "target" / "debug" / "api-analyzer",
        workspace_root / "target" / "release" / "api-analyzer",
        # When installed as a package
        package_dir / "bin" / "api-analyzer",
    ]

    # Add extension for Windows
    if platform.system() == "Windows":
        possible_locations = [
            loc.with_suffix(".exe") for loc in possible_locations
        ] + possible_locations

    for location in possible_locations:
        if location.exists() and os.access(location, os.X_OK):
            return location

    # Try absolute paths that might work in development environment
    absolute_paths = [
        Path("/workspaces/ruff/target/debug/api-analyzer"),
        Path("/workspaces/ruff/target/release/api-analyzer"),
    ]

    for location in absolute_paths:
        if location.exists() and os.access(location, os.X_OK):
            return location

    # List directories to help debug
    if os.environ.get("PUBSCAN_DEBUG"):
        print("Debug: Looking for binary in following locations:", file=sys.stderr)
        for location in possible_locations + absolute_paths:
            status = "EXISTS" if location.exists() else "NOT FOUND"
            print(f"  - {location}: {status}", file=sys.stderr)

        print(f"Debug: Current directory is: {Path.cwd()}", file=sys.stderr)
        print(f"Debug: Package directory is: {package_dir}", file=sys.stderr)
        print(f"Debug: Workspace root is: {workspace_root}", file=sys.stderr)

    return None


def run_analyzer(args: List[str]) -> int:
    """Run the API analyzer with the given args."""
    binary = find_binary()
    if not binary:
        print("Error: Could not find the api-analyzer binary.", file=sys.stderr)
        print(
            "Please ensure the package is installed correctly or the binary is built.",
            file=sys.stderr,
        )
        print("Try running: cargo build -p ruff_api_analyzer", file=sys.stderr)
        print("Set PUBSCAN_DEBUG=1 for more information.", file=sys.stderr)
        return 1

    # Run the binary with the provided args
    result = subprocess.run([str(binary)] + args)
    return result.returncode


def main(args: Optional[List[str]] = None) -> int:
    """Run the pubscan CLI."""
    if args is None:
        args = sys.argv[1:]

    parser = argparse.ArgumentParser(
        prog="pubscan", description="Analyze a Python module's public API surface area"
    )
    parser.add_argument("--version", action="version", version=f"pubscan {__version__}")

    # Parse just --version, otherwise pass all args to the binary
    if len(args) == 1 and args[0] in ("--version", "-V"):
        parser.parse_args(args)
        return 0

    return run_analyzer(args)


if __name__ == "__main__":
    sys.exit(main())
