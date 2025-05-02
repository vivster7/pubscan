# pubscan

A Python tool for analyzing the public API of Python modules or packages.

## Installation

Once the package is available on PyPI, you can install it with:

```bash
pip install pubscan
```

For now, you can use the provided wrapper script:

```bash
# Build the Rust binary first
cd /workspaces/ruff
cargo build -p ruff_api_analyzer

# Then run the wrapper script
cd crates/ruff_api_analyzer/python
./pubscan_cmd.py --help
```

## Usage

```bash
# Basic usage
pubscan PATH_TO_MODULE

# Specify project root (helps analyzer find imports)
pubscan PATH_TO_MODULE --project-root PROJECT_ROOT

# Output JSON format
pubscan PATH_TO_MODULE --output-format json

# Increase verbosity for debugging
pubscan PATH_TO_MODULE -v
```

## Description

The API Analyzer scans Python modules to identify their "public API" - defined as symbols from the target module that are used by external files. This helps you understand which parts of your codebase are actually being used externally, making it easier to maintain backward compatibility.

## Examples

Analyze a single Python file:

```bash
pubscan path/to/file.py
```

Analyze a Python package:

```bash
pubscan path/to/package/
```

Analyze with project root specified:

```bash
pubscan path/to/package/ --project-root /path/to/project
```

## Development

To work on this package:

1. Build the Rust binary:

   ```bash
   cd /workspaces/ruff
   cargo build -p ruff_api_analyzer
   ```

2. Run the wrapper script:

   ```bash
   cd crates/ruff_api_analyzer/python
   ./pubscan_cmd.py [args]
   ```

3. For development installation:
   ```bash
   cd crates/ruff_api_analyzer/python
   pip install -e .
   ```

For publishing packages to PyPI, see the detailed [Publishing Guide](./PUBLISHING.md).

## Publishing to PyPI

To publish this package to PyPI:

1. **Prerequisites**:

   - You'll need a PyPI account, registered at https://pypi.org/account/register/
   - Install required tools: `pip install build twine`
   - Make sure you have the latest setuptools: `pip install --upgrade setuptools wheel`

2. **Update Version**:

   - Update the version in `pubscan/__init__.py`
   - Make sure all changes are documented

3. **Build the Distribution**:

   - Build the Rust binary first:
     ```bash
     cd /workspaces/ruff
     cargo build --release -p ruff_api_analyzer
     ```
   - Then build the Python package:
     ```bash
     cd crates/ruff_api_analyzer/python
     python -m build
     ```
   - This will create both a source distribution (.tar.gz) and a wheel (.whl) in the `dist/` directory

4. **Test the Package** (optional but recommended):

   - Upload to TestPyPI first:
     ```bash
     twine upload --repository-url https://test.pypi.org/legacy/ dist/*
     ```
   - Install from TestPyPI:
     ```bash
     pip install --index-url https://test.pypi.org/simple/ --no-deps pubscan
     ```
   - Test that the installation works

5. **Upload to PyPI**:

   - Upload the package:
     ```bash
     twine upload dist/*
     ```
   - You'll be prompted for your PyPI credentials

6. **Verify the Upload**:
   - Check the package page on PyPI: https://pypi.org/project/pubscan/
   - Test the installation from PyPI:
     ```bash
     pip install pubscan
     ```

For subsequent releases, repeat steps 2-6.

## Output Example

### Text Output

```
Public API for /path/to/package:

CLASSES:
  PublicClass (3 external usages, public)
    Fully qualified: my_package.PublicClass
    This is a public class.
    Location: /path/to/package/module.py
    Imported by: /path/to/client1.py, /path/to/client2.py, /path/to/client3.py

FUNCTIONS:
  public_function (2 external usages, public)
    Fully qualified: my_package.public_function
    This is a public function.
    Location: /path/to/package/module.py
    Imported by: /path/to/client1.py, /path/to/client2.py

Found 2 public API symbols with external usage.
```

### JSON Output

The JSON output provides the same information in a machine-readable format, useful for integration with other tools or automation.

## License

MIT
