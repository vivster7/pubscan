# Ruff API Analyzer

A standalone command-line tool for analyzing the public API of Python modules or packages.

## Description

The API Analyzer scans Python modules to identify their "public API" - defined as symbols from the target module that are used by external files. This helps you understand which parts of your codebase are actually being used externally, making it easier to maintain backward compatibility.

## Building

To build the API analyzer:

```bash
# Build with debug configuration
cargo build -p ruff_api_analyzer

# Build with release configuration (recommended for production use)
cargo build -p ruff_api_analyzer --release
```

The binary will be created at:

- Debug: `target/debug/api-analyzer`
- Release: `target/release/api-analyzer`

## Python Package

The `ruff_api_analyzer` crate also includes a Python package called `pubscan` that wraps the Rust binary. This provides a simpler installation experience for Python users who can install it with pip:

```bash
# Navigate to the Python package directory
cd crates/ruff_api_analyzer/python

# Install in development mode
pip install -e .

# Or build and install the package
pip install .
```

Once installed, you can use the Python wrapper:

```bash
pubscan path/to/module
```

See the [Python package README](python/README.md) for more information.

## Usage

```bash
# Basic usage
api-analyzer PATH_TO_MODULE

# Specify project root (helps analyzer find imports)
api-analyzer PATH_TO_MODULE --project-root PROJECT_ROOT

# Output JSON format
api-analyzer PATH_TO_MODULE --output-format json

# Increase verbosity for debugging
api-analyzer PATH_TO_MODULE -v
```

## Examples

Analyze a single Python file:

```bash
api-analyzer path/to/file.py
```

Analyze a Python package:

```bash
api-analyzer path/to/package/
```

Analyze with project root specified:

```bash
api-analyzer path/to/package/ --project-root /path/to/project
```

## Command-Line Options

```
Usage: api-analyzer [OPTIONS] <TARGET>

Arguments:
  <TARGET>  The path to the Python module (.py file) or package (directory) to analyze

Options:
  -o, --output-format <FORMAT>     The output format to use (text/json) [default: text]
      --python <PATH>              The path to the Python executable to use for venv parsing
      --project-root <PATH>        Explicitly specify the project root directory (default: auto-detected from target)
      --no-parallel                Disable parallel processing for file analysis
  -v, --verbose...                 Increase verbosity (can be used multiple times)
  -h, --help                       Print help
  -V, --version                    Print version
```

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
