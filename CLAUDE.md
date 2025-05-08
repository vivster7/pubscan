# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Pubscan (also known as Ruff API Analyzer) is a tool that analyzes the public API of Python modules or packages. It identifies the "effective public surface area" of packages, showing which symbols are being used by external code. This helps package maintainers understand what parts of their code are actually being used and assists new developers in identifying important parts of a codebase.

## Building and Running

### Building from Source

```bash
# Build with debug configuration
cargo build -p ruff_api_analyzer

# Build with release configuration (recommended for production)
cargo build -p ruff_api_analyzer --release
```

The binary will be created at:
- Debug: `target/debug/api-analyzer`
- Release: `target/release/api-analyzer`

### Rust Binary Usage

```bash
# Basic usage
api-analyzer PATH_TO_MODULE

# Output JSON format
api-analyzer PATH_TO_MODULE --output-format json

# Use short output format
api-analyzer PATH_TO_MODULE --short

# Specify project root (helps analyzer find imports)
api-analyzer PATH_TO_MODULE --project-root PROJECT_ROOT

# Increase verbosity for debugging
api-analyzer PATH_TO_MODULE -v

# Don't exclude test files from analysis
api-analyzer PATH_TO_MODULE --no-ignore-test-files
```

### Python Package

The project includes a Python wrapper (`pubscan`) for the Rust binary:

```bash
# Navigate to the Python package directory
cd crates/ruff_api_analyzer/python

# Install in development mode
pip install -e .

# Use the Python wrapper
pubscan PATH_TO_MODULE
```

## Testing

```bash
# Run all tests
cargo test

# Run specific tests for API analyzer
cargo test -p ruff analyze_api

# Test the Python wrapper
cd crates/ruff_api_analyzer/python
python -m unittest test_pubscan.py
```

## Project Architecture

The project is structured as a Rust workspace with multiple crates. The main components are:

1. **ruff_api_analyzer** - The main crate that provides the API analyzer functionality.
   - `src/main.rs` - Entry point for the standalone binary
   - `python/` - Python package wrapper around the Rust binary

2. **ruff** - The core package that contains the analyze_api command implementation.
   - `crates/ruff/src/commands/analyze_api.rs` - Core implementation of the API analysis

3. **Other supporting crates** - The project leverages various components from the ruff linter.

### Key Components

- **ApiAnalyzer** - The main struct that manages the analysis process, tracking candidate symbols and their usage.
- **Symbol Tracking** - The system tracks both imports and actual usage (call sites) of symbols.
- **Output Formats** - The analyzer supports text, short, and JSON output formats.

### Analysis Process

1. Identify the target module/package and project root
2. Collect all Python files in the project
3. Divide files into target files and external files
4. Extract candidate symbols from target files
5. Analyze external files to determine which symbols are imported and used
6. Output the results in the requested format

## Development Workflow

1. Make changes to the Rust code
2. Build the binary with `cargo build -p ruff_api_analyzer`
3. Test your changes with appropriate test cases
4. If modifying the Python wrapper, update the Python tests accordingly

The project follows Rust's conventions for code organization and testing.