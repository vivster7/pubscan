"""Setup script for pubscan."""

from __future__ import annotations

import os
import platform
import re
import subprocess
import sys
from pathlib import Path

from setuptools import find_packages, setup
from setuptools.command.build_py import build_py
from setuptools.command.develop import develop


# Read the version from __init__.py
def get_version():
    init_path = Path(__file__).parent / "pubscan" / "__init__.py"
    with open(init_path, "r") as f:
        content = f.read()
    match = re.search(r'__version__\s*=\s*["\']([^"\']+)["\']', content)
    if match:
        return match.group(1)
    raise RuntimeError("Cannot find version information")


class BuildRustBinary(build_py):
    """Custom build command to build the Rust binary before packaging."""

    def run(self):
        # Ensure the binary directory exists
        binary_dir = Path(__file__).parent / "pubscan" / "bin"
        binary_dir.mkdir(exist_ok=True)

        # Binary name depends on platform
        binary_name = "api-analyzer"
        if platform.system() == "Windows":
            binary_name += ".exe"

        # Check if binary already exists or if we're in cibuildwheel
        binary_path = binary_dir / binary_name
        is_cibuildwheel = os.environ.get("CIBUILDWHEEL", "0") == "1"

        if is_cibuildwheel:
            # When in cibuildwheel, we don't need to build the binary here
            # It will be built by cibuildwheel in the CIBW_BEFORE_BUILD step
            print("Running in cibuildwheel environment, skipping binary build")
        elif not binary_path.exists():
            # Build the Rust binary
            self._build_rust_binary()
            # Copy the binary to the package directory
            self._copy_binary(binary_dir)
        else:
            print(f"Binary already exists at {binary_path}, skipping build")

        # Run the original build_py command
        super().run()

    def _build_rust_binary(self):
        """Build the Rust binary using cargo."""
        # Try different paths to find the crate root
        possible_crate_paths = [
            Path(__file__).parent.parent,  # Direct parent
            Path(__file__).parent.parent.parent.parent
            / "crates"
            / "ruff_api_analyzer",  # In workspace
        ]

        crate_path = None
        for path in possible_crate_paths:
            cargo_toml = path / "Cargo.toml"
            if cargo_toml.exists():
                with open(cargo_toml, "r") as f:
                    if "ruff_api_analyzer" in f.read():
                        crate_path = path
                        break

        if not crate_path:
            print("Error: Could not find ruff_api_analyzer crate", file=sys.stderr)
            print(
                f"Searched in: {', '.join(str(p) for p in possible_crate_paths)}",
                file=sys.stderr,
            )
            sys.exit(1)

        print(f"Building Rust binary from crate at: {crate_path}")
        cmd = ["cargo", "build", "--release", "-p", "ruff_api_analyzer"]

        try:
            subprocess.check_call(
                cmd, cwd=crate_path.parent.parent
            )  # Run from workspace root
            print("Successfully built Rust binary")
        except subprocess.CalledProcessError as e:
            print(f"Error building Rust binary: {e}", file=sys.stderr)
            raise

    def _copy_binary(self, dest_dir):
        """Copy the binary to the package directory."""
        # Look for the binary in multiple possible locations
        possible_locations = [
            Path(__file__).parent.parent / "target" / "release",
            Path(__file__).parent.parent.parent.parent / "target" / "release",
        ]

        # Binary name depends on platform
        binary_name = "api-analyzer"
        if platform.system() == "Windows":
            binary_name += ".exe"

        # Find the binary
        binary_path = None
        for location in possible_locations:
            path = location / binary_name
            if path.exists():
                binary_path = path
                break

        if not binary_path:
            paths_str = "\n  - ".join(
                [str(p / binary_name) for p in possible_locations]
            )
            print(
                f"Error: Could not find built binary. Looked in:\n  - {paths_str}",
                file=sys.stderr,
            )
            sys.exit(1)

        print(f"Found binary at: {binary_path}")

        # Copy the binary
        import shutil

        dest = dest_dir / binary_name
        shutil.copy2(binary_path, dest)
        print(f"Copied binary to: {dest}")

        # Make the binary executable on Unix
        if platform.system() != "Windows":
            os.chmod(dest, 0o755)


class DevelopCommand(develop):
    """Custom develop command to build the Rust binary for development installs."""

    def run(self):
        # Skip binary build if we're in cibuildwheel
        is_cibuildwheel = os.environ.get("CIBUILDWHEEL", "0") == "1"
        if is_cibuildwheel:
            print("Running in cibuildwheel environment, skipping binary build")
            super().run()
            return

        # Build the binary
        build_cmd = BuildRustBinary(self.distribution)
        build_cmd._build_rust_binary()

        # Create bin directory
        binary_dir = Path(__file__).parent / "pubscan" / "bin"
        binary_dir.mkdir(exist_ok=True)

        # Copy the binary for development mode
        build_cmd._copy_binary(binary_dir)

        # Run the original develop command
        super().run()


setup(
    name="pubscan",
    version=get_version(),
    description="A tool for analyzing Python module's public API surface area",
    long_description=open(Path(__file__).parent / "README.md").read(),
    long_description_content_type="text/markdown",
    author="Ruff Developers",
    author_email="info@ruff.rs",
    url="https://github.com/astral-sh/ruff",
    packages=find_packages(),
    include_package_data=True,
    package_data={"pubscan": ["bin/*"]},
    entry_points={
        "console_scripts": [
            "pubscan=pubscan.cli:main",
        ],
    },
    cmdclass={
        "build_py": BuildRustBinary,
        "develop": DevelopCommand,
    },
    classifiers=[
        "Development Status :: 4 - Beta",
        "Intended Audience :: Developers",
        "License :: OSI Approved :: MIT License",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.7",
        "Programming Language :: Python :: 3.8",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
        "Topic :: Software Development :: Libraries :: Python Modules",
        "Topic :: Software Development :: Quality Assurance",
    ],
    python_requires=">=3.7",
)
