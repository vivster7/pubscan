"""Command-line interface for pubscan."""

from __future__ import annotations

import argparse
import os
import platform
import subprocess
import sys
from pathlib import Path
from typing import List, Optional

from pubscan import __version__


def find_binary() -> Optional[Path]:
    """Find the api-analyzer binary."""
    # Look in common locations
    package_dir = Path(__file__).parent
    crate_dir = (
        package_dir.parent.parent.parent
    )  # Go up to the ruff_api_analyzer crate root

    # Check if we're in development mode or installed
    possible_locations = [
        # Development locations - navigate to crate target directories
        crate_dir / "target" / "debug" / "api-analyzer",
        crate_dir / "target" / "release" / "api-analyzer",
        # When installed as a package
        package_dir / "bin" / "api-analyzer",
        # Nested in the workspace structure
        crate_dir.parent.parent / "target" / "debug" / "api-analyzer",
        crate_dir.parent.parent / "target" / "release" / "api-analyzer",
    ]

    # Add extension for Windows
    if platform.system() == "Windows":
        possible_locations = [
            loc.with_suffix(".exe") for loc in possible_locations
        ] + possible_locations

    for location in possible_locations:
        if location.exists() and os.access(location, os.X_OK):
            print(f"Found binary at: {location}", file=sys.stderr)
            return location

    # List directories to help debug
    print("Debug: Looking for binary in following locations:", file=sys.stderr)
    for location in possible_locations:
        status = "EXISTS" if location.exists() else "NOT FOUND"
        print(f"  - {location}: {status}", file=sys.stderr)

    print(f"Debug: Current directory is: {Path.cwd()}", file=sys.stderr)
    print(f"Debug: Package directory is: {package_dir}", file=sys.stderr)

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
