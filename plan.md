# Usage-Based Public API Analysis for Python Modules/Packages

## Overview

This tool analyzes a Python module or package to determine its effective public API based on _actual usage_ in external code within the project. Unlike traditional definitions relying on naming conventions (`_` prefix) or `__all__` declarations, this analysis defines a symbol as "public" if:

1. It is defined within the target module file or package directory
2. It is imported and used by at least one other Python file that is external to the target boundary

## Usage

```bash
ruff analyze api <path/to/module.py_or_package_dir> [--output-format json] ...
```

## Implementation Status

### ✅ Core Features (Completed)

- **Command-line Interface**: `ruff analyze api` with appropriate argument parsing
- **Target Boundary Detection**: Module files vs. package directories
- **Symbol Extraction**: Parse Python files and extract symbols from AST
- **External Usage Analysis**: Detect which symbols are used externally with accurate counting
- **Results Output**: Both text and JSON formats with symbol details by type
- **Test Coverage**: Multiple scenarios (modules, packages, classes, `__all__` definitions)

### ✅ Recent Improvements (Completed)

- **AST-based Analysis**: Clean semantic analysis using Ruff's Python parser
- **Code Organization**: Improved structure with clear sections and type aliases
- **Logging**: Replaced debug printouts with proper logging at appropriate levels
- **Module.Symbol Detection**: Enhanced detection of package access patterns
- **Resolved False Positives**: Fixed variable collision issues and proper module tracking
- **Explicit Project Root**: Added option to specify project root via `--project-root` parameter
- **Expanded Target Support**: Improved handling of target modules outside the project root
- **Parallel Processing**: Added parallel file analysis with Rayon and `--no-parallel` option for small projects
- **Bug Fix: Fully Qualified Name Matching**: Fixed a bug where imports with different module paths (e.g., `from a.b import mysymbol`) were incorrectly matching symbols with unrelated fully qualified names (e.g., `x.y.mysymbol`). Now fully qualified names are properly validated during import processing.

### 🔄 Code Quality Enhancements (Completed)

- **Unified AST Traversal**: Consolidated redundant traversal logic
- **Type Aliases**: Added for complex types to improve readability
- **Documentation**: Improved function and section documentation
- **Modular Design**: Functionally organized related logic together
- **Clear Section Boundaries**: Added section comments for better navigation

### 🚧 Future Work (In Progress)

1. **Enhanced Test Coverage**

   - Additional edge cases for import patterns
   - Complex package structures with nested imports

2. **Performance Optimization**

   - ✅ **Parallel Processing**: Implemented parallelization of external file analysis using Rayon
     - Two-phase approach: serial symbol collection + parallel usage analysis
     - Process external files concurrently with thread-safe result aggregation
     - Thread-safe data structures with Arc and Mutex for shared state
     - Added `--no-parallel` flag to disable parallelism when needed
     - Benchmark testing shows sequential processing can be faster for small projects
     - Similar to approach used in `analyze_graph` command
   - **Incremental Analysis**: Cache analysis results for files that haven't changed
   - **Targeted Analysis**: Analyze only files that import from the target module
   - **Optimized AST Traversal**: Reduce unnecessary recursive traversals

3. **Architectural Improvements**

   - ✅ **Encapsulate Analysis State**: Created `ApiAnalyzer` and `FileAnalysisState` structs to hold shared and per-file state, reducing function parameter bloat
   - ✅ **Utilize the Visitor Pattern**: Implemented `AstVisitor` trait with `ApiAnalyzerVisitor` to replace manual AST traversal, improving maintainability and completeness
   - ✅ **Separate Analysis Phases**: Broke down the analysis into discrete phases (setup, categorization, extraction, analysis, formatting)
   - ✅ **Simplify Parallelism Logic**: Extracted file processing into a separate function to eliminate duplication between sequential and parallel paths
   - ✅ **Re-evaluate Import Handling**: Separated import tracking from usage counting for cleaner code organization
   - ⚠️ **Improve Fully Qualified Name Handling**: Create more robust FQN matching logic, potentially leveraging Ruff's resolver
   - ✅ **Enhance Data Structures**: Replaced complex nested generics with more specific and descriptive types
   - ✅ **Review Error Handling**: Improved error collection and reporting strategy for better user experience
   - **Command Pattern**: Break down the main `analyze_api` function into specialized command objects
   - **Output Formatting Strategy Pattern**: Create a strategy pattern with different formatters
   - **File Repository Abstraction**: Centralize file operations behind a repository interface
   - **Configuration Builder Pattern**: Improve configuration handling with a builder pattern
   - **Implementation Priority**: Command Pattern → Output Formatting Strategy → File Repository → Configuration Builder → FQN Handling

4. **Feature Enhancements**

   - More detailed usage information (read vs. write access)
   - API dependency graphs between components
   - Documentation template generation
   - Additional output formats (markdown, HTML)

5. **User Experience**

   - More filtering and sorting options
   - Interactive mode for exploring APIs
   - Integration with documentation tools

6. **Command-Line Interface Improvements**
   - Include/exclude patterns for targeting specific files or directories
   - Verbosity controls specific to API analysis
   - Support for custom output file paths

## Architecture

The implementation leverages Ruff's existing infrastructure:

- **Parsing**: `ruff_python_parser` for generating ASTs
- **File Discovery**: `ruff_workspace` and `resolver` to find Python files
- **Configuration**: Reuse of Ruff's configuration system
- **Parallelism**: Rayon library for parallel processing (planned)

The analysis follows these high-level steps:

1. **Target Identification**: Determine the target boundary (file or package)
2. **Symbol Extraction**: Find all symbols defined in the target
3. **External Usage Analysis**: Analyze imports and usage in external files (parallelizable)
4. **Public API Construction**: Build final API based on actual usage

## Data Structures

```rust
// Symbol kinds we can detect
enum SymbolKind {
    Function,
    Class,
    Variable,
    Module,
    Other,
}

// Information about a symbol defined in the target
struct DefinedSymbol {
    kind: SymbolKind,
    location: PathBuf,
    docstring: Option<String>,
    is_public: bool,
    fully_qualified_name: String,
}

// Information about a symbol's usage
struct ApiSymbol {
    name: String,
    definition: DefinedSymbol,
    usage_count: usize,
    importers: HashSet<PathBuf>,
}
```
