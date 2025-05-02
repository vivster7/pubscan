# Publishing Guide for `pubscan`

This document provides detailed instructions for maintainers on how to release new versions of the `pubscan` package to PyPI.

## Prerequisites

- A PyPI account with access to the `pubscan` project
- Python 3.7+ installed
- Required tools:
  ```bash
  pip install build twine setuptools wheel
  ```

## Release Process

### 1. Prepare the Codebase

1. **Update Rust Changes First**:

   - Make sure all necessary changes to the Rust code are committed
   - Ensure the `api-analyzer` binary builds correctly:
     ```bash
     cargo build --release -p ruff_api_analyzer
     ```

2. **Update the Version Number**:

   - Update `__version__` in `pubscan/__init__.py` following [SemVer](https://semver.org/) principles:
     - MAJOR version for incompatible API changes
     - MINOR version for backward-compatible functionality additions
     - PATCH version for backward-compatible bug fixes

3. **Update Documentation**:

   - Update the README.md with any new features or changes
   - Make sure all examples are up-to-date

4. **Run Tests**:
   - Test that the package works correctly:
     ```bash
     cd crates/ruff_api_analyzer/python
     # Test the wrapper directly
     ./pubscan_cmd.py --help
     # Test the binary functionality with an actual module
     ./pubscan_cmd.py /path/to/test/module
     ```

### 2. Build the Package

1. **Build the Rust Binary First**:

   ```bash
   cd /workspaces/ruff
   cargo build --release -p ruff_api_analyzer
   ```

2. **Build the Python Distribution**:

   ```bash
   cd crates/ruff_api_analyzer/python
   python -m build
   ```

   This will create:

   - Source distribution (`.tar.gz`) in the `dist/` directory
   - Wheel (`.whl`) in the `dist/` directory

3. **Verify the Build**:
   - Check that both files are in the `dist/` directory
   - Ensure the binaries are included in the distribution by inspecting the archive content

### 3. Test the Package (Optional but Recommended)

1. **Upload to TestPyPI**:

   ```bash
   twine upload --repository-url https://test.pypi.org/legacy/ dist/*
   ```

   You'll be prompted for your TestPyPI credentials.

2. **Create a Clean Test Environment**:

   ```bash
   python -m venv /tmp/test-pubscan
   source /tmp/test-pubscan/bin/activate
   ```

3. **Install from TestPyPI**:

   ```bash
   pip install --index-url https://test.pypi.org/simple/ --no-deps pubscan
   ```

4. **Test the Installation**:

   ```bash
   pubscan --help
   ```

5. **Clean Up Test Environment**:
   ```bash
   deactivate
   rm -rf /tmp/test-pubscan
   ```

### 4. Upload to PyPI

1. **Upload with Twine**:

   ```bash
   twine upload dist/*
   ```

   You'll be prompted for your PyPI credentials.

2. **Verify the Upload**:

   - Visit https://pypi.org/project/pubscan/ to confirm the new version is available
   - Check that the package description, README, and metadata look correct

3. **Test the Published Package**:
   ```bash
   pip install --upgrade pubscan
   pubscan --help
   ```

## Cross-Platform Distribution

For production-quality releases, you should build platform-specific wheels for various operating systems. Here are approaches for handling this:

### Using GitHub Actions for Multiple Platforms

1. **Create a GitHub Actions Workflow**:
   Create a file `.github/workflows/build-wheels.yml` that builds wheels for multiple platforms:

   ```yaml
   name: Build wheels

   on:
     release:
       types: [created]
     workflow_dispatch:

   jobs:
     build_wheels:
       name: Build ${{ matrix.os }} wheel
       runs-on: ${{ matrix.os }}
       strategy:
         matrix:
           os: [ubuntu-latest, macos-latest, windows-latest]
           python-version: ["3.8", "3.9", "3.10", "3.11"]

       steps:
         - uses: actions/checkout@v3

         - name: Set up Python
           uses: actions/setup-python@v4
           with:
             python-version: ${{ matrix.python-version }}

         - name: Install Rust
           uses: actions-rs/toolchain@v1
           with:
             profile: minimal
             toolchain: stable

         - name: Build wheels
           run: |
             python -m pip install --upgrade pip
             pip install build wheel
             cd crates/ruff_api_analyzer/python
             python -m build

         - name: Upload wheels
           uses: actions/upload-artifact@v3
           with:
             name: wheels-${{ matrix.os }}-py${{ matrix.python-version }}
             path: crates/ruff_api_analyzer/python/dist/*.whl
   ```

2. **Using cibuildwheel for More Control**:
   For more complex builds, consider using [cibuildwheel](https://github.com/pypa/cibuildwheel):

   ```yaml
   - name: Build wheels
     uses: pypa/cibuildwheel@v2.12.1
     env:
       CIBW_BUILD: cp38-* cp39-* cp310-* cp311-*
       CIBW_BEFORE_BUILD: "cd crates/ruff_api_analyzer && cargo build --release -p ruff_api_analyzer"
   ```

### Building ARM64 Wheels

For M1/M2 Macs and other ARM architectures:

1. **Use GitHub Actions with ARM Runners**:

   ```yaml
   strategy:
     matrix:
       os: [ubuntu-latest, macos-latest, windows-latest, ubuntu-arm64, macos-arm64]
   ```

2. **Cross-compilation**:
   You can also use cross-compilation with tools like `cross`:
   ```bash
   cargo install cross
   cross build --target aarch64-unknown-linux-gnu --release
   ```

### Universal2 Wheels for macOS

For both Intel and Apple Silicon:

```bash
# Install dependencies
pip install setuptools wheel delocate

# Build the wheel
python setup.py bdist_wheel

# Convert to universal2
delocate-wheel -w dist --require-universal2 dist/*.whl
```

### 5. Post-Release Tasks

1. **Tag the Release in Git**:

   ```bash
   # From the repository root
   git tag -a pubscan-v0.1.0 -m "Release pubscan v0.1.0"
   git push origin pubscan-v0.1.0
   ```

2. **Update Development Version**:

   - After releasing (e.g., v0.1.0), update `__version__` to the next development version (e.g., v0.1.1-dev0)

3. **Announce the Release**:
   - In the Ruff discord or other appropriate channels
   - Update relevant documentation sites

## Troubleshooting

### Binary Not Found in the Package

If the binary isn't found in the installed package:

1. Check `MANIFEST.in` to ensure it includes the binary directory
2. Verify that `setup.py` correctly specifies `package_data`
3. Make sure the binary is being built and copied to the right location before packaging

### Upload Issues

If you have issues with PyPI uploads:

1. Check the error messages from `twine`
2. Verify your PyPI account has the necessary permissions
3. Try uploading to TestPyPI first to diagnose issues

### Installation Issues

If users report installation problems:

1. Check if the issue is platform-specific
2. Verify the wheel is correctly built for the target platforms
3. Consider using GitHub Actions to build and test the package on multiple platforms
