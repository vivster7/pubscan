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
    """Custom build command to build the Rust binary."""

    def _build_rust_binary(self):
        """Build the Rust binary."""
        # If CIBUILDWHEEL is set, we're running under cibuildwheel
        cibuildwheel = os.environ.get("CIBUILDWHEEL") == "1"
        bin_dir = Path(__file__).parent / "pubscan" / "bin"
        bin_dir.mkdir(exist_ok=True)

        # Figure out the output binary path
        binary_name = (
            "api-analyzer.exe" if platform.system() == "Windows" else "api-analyzer"
        )
        output_path = bin_dir / binary_name

        # Check if the binary already exists (pre-copied during development)
        if output_path.exists():
            print(f"Binary already exists at {output_path}")
            return

        # Don't build from source if we're running under cibuildwheel since it should be using the
        # pre-built binary from the before-build step
        if cibuildwheel:
            print("Skipping Rust binary build in cibuildwheel environment.")
            return

        # Determine the path to the Rust crate
        crate_path = Path(__file__).parent.parent.parent

        # Run cargo build to create the binary
        print(f"Building Rust binary from {crate_path}...")
        cargo_cmd = ["cargo", "build", "--release", "--package", "ruff_api_analyzer"]

        try:
            subprocess.run(cargo_cmd, cwd=crate_path, check=True)
        except subprocess.CalledProcessError as e:
            print(f"Failed to build Rust binary: {e}")
            raise

        # Determine the path to the built binary
        target_dir = crate_path / "target" / "release"
        binary_path = target_dir / binary_name

        # Copy the binary to the package directory
        print(f"Copying binary from {binary_path} to {output_path}")
        # Handle non-existent binary with the correct name in Windows
        if not binary_path.exists() and platform.system() == "Windows":
            binary_path = target_dir / "api-analyzer.exe"

        if not binary_path.exists():
            binary_path = target_dir / "api-analyzer"

        if not binary_path.exists():
            raise FileNotFoundError(f"Rust binary not found at {binary_path}")

        # Use shell command to copy to preserve executable permissions
        if platform.system() == "Windows":
            subprocess.run(
                ["copy", str(binary_path), str(output_path)], shell=True, check=True
            )
        else:
            subprocess.run(["cp", str(binary_path), str(output_path)], check=True)

    def run(self):
        self._build_rust_binary()
        super().run()


class DevelopRustBinary(develop):
    """Custom develop command to build the Rust binary in development mode."""

    def run(self):
        cmd = BuildRustBinary(self.distribution)
        cmd._build_rust_binary()
        super().run()


# Get the long description from the README file
long_description = (Path(__file__).parent / "README.md").read_text(encoding="utf-8")

# Make sure bin directory exists with __init__.py so it's treated as a package
bin_dir = Path(__file__).parent / "pubscan" / "bin"
bin_dir.mkdir(exist_ok=True)
init_file = bin_dir / "__init__.py"
if not init_file.exists():
    init_file.write_text("# Binary directory\n")

setup(
    name="pubscan",
    version=get_version(),
    description="A tool for analyzing Python module's public API surface area",
    long_description=long_description,
    long_description_content_type="text/markdown",
    author="Ruff Developers",
    author_email="info@ruff.rs",
    url="https://github.com/astral-sh/ruff",
    packages=find_packages(),
    include_package_data=True,
    package_data={
        "pubscan": ["bin/*"],
    },
    # Add explicit data_files to ensure binaries are included
    data_files=[
        (
            "pubscan/bin",
            [
                str(
                    Path(
                        "pubscan/bin/api-analyzer.exe"
                        if platform.system() == "Windows"
                        else "pubscan/bin/api-analyzer"
                    )
                )
            ],
        )
    ],
    cmdclass={
        "build_py": BuildRustBinary,
        "develop": DevelopRustBinary,
    },
    entry_points={
        "console_scripts": [
            "pubscan=pubscan.cli:main",
        ],
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
