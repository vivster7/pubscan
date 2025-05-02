#!/usr/bin/env python3
"""Standalone command script for pubscan."""

from __future__ import annotations

import os
import platform
import subprocess
import sys
from pathlib import Path


def find_binary():
    """Find the api-analyzer binary."""
    # First try in local workspace
    workspace_paths = [
        Path("/workspaces/ruff/target/debug/api-analyzer"),
        Path("/workspaces/ruff/target/release/api-analyzer"),
    ]

    # Add extension for Windows
    if platform.system() == "Windows":
        workspace_paths = [
            path.with_suffix(".exe") for path in workspace_paths
        ] + workspace_paths

    for path in workspace_paths:
        if path.exists() and os.access(path, os.X_OK):
            return path

    return None


def main():
    """Run pubscan."""
    binary = find_binary()
    if not binary:
        print("Error: Could not find the api-analyzer binary.", file=sys.stderr)
        print(
            "Please build it using: cargo build -p ruff_api_analyzer", file=sys.stderr
        )
        return 1

    # Run the binary with all args
    result = subprocess.run([str(binary)] + sys.argv[1:])
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
