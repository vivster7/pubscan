#!/usr/bin/env python3
"""Test script for pubscan."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

# Try to find the binary directly
workspace_root = Path(__file__).parent.parent.parent.parent
possible_locations = [
    workspace_root / "target" / "debug" / "api-analyzer",
    workspace_root / "target" / "release" / "api-analyzer",
]

binary_path = None
for location in possible_locations:
    if location.exists():
        print(f"Found binary at: {location}")
        binary_path = location
        break

if binary_path:
    print(f"Running {binary_path}")
    args = sys.argv[1:]
    sys.exit(subprocess.call([str(binary_path)] + args))
else:
    print("Could not find api-analyzer binary. Please build with:")
    print("cargo build -p ruff_api_analyzer")
    sys.exit(1)
