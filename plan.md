# Plan: Usage-Based Public API Analysis for Python Modules/Packages

## Goal

To implement a Ruff command that analyzes a given Python module (`.py` file) or package (directory) to determine its effective public API based on _actual usage_ from external code within the project. The command should report the public symbols, their definition details, and how many times each is used externally.

## Command Example

The intended command-line interface would be:

```bash
ruff analyze api <path/to/module.py_or_package_dir> [--output-format json] ...
```

## Core Idea: Usage-Based Public API

Unlike traditional definitions relying on naming conventions (`_` prefix) or `__all__`, this analysis defines a symbol as "public" if and only if:

1.  It is defined within the target module file or package directory.
2.  It is imported and used by _at least one other Python file_ that is **external** to the target module/package boundary.

## Progress Checkpoint (Implementation Status)

### Completed Features

1. **Command-line Interface**: Implemented the `ruff analyze api` command with appropriate argument parsing.
2. **Target Boundary Detection**: Implemented logic to identify and process module files vs. package directories.
3. **Symbol Extraction with AST Analysis**: Parse Python files using Ruff's Python parser and extract symbols from the AST.
4. **External Usage Analysis**: Implemented import detection to identify which symbols are used externally with accurate counting.
5. **Results Output**: Provided both text and JSON output formats with relevant symbol details, organized by symbol types.
6. **Comprehensive Tests**: Created tests covering various scenarios (single modules, packages, classes, `__all__` definitions).
7. **Code Quality**: Removed unused code, fixed linting warnings, and ensured a clean codebase.

### Recent Improvements

1. **AST-based Analysis**: Transitioned from text-based parsing to AST analysis using the `ruff_python_parser`.
2. **Reliable External Usage**: Fixed calculation of external usage, avoiding incorrect counting.
3. **Code Cleanup**: Removed unused code and improved the structure.
4. **Output Formatting**: Enhanced the formatting of the output for better readability.
5. **Fixed Symbol Tracking**: Corrected the AST access patterns, improved string handling, and refined symbol usage tracking.
6. **Module.Symbol Detection**: Added detection for nested package access patterns like `mypackage.core.multiply()`.
7. **Fixed Variable Collision Issues**: Resolved false positives where variables with the same name in different modules (like `logger`) were incorrectly counted as API usage.
8. **Improved Module.Symbol Detection**: Enhanced detection of module.symbol access patterns when using aliased imports like `from a import b` followed by `b.logger`.

### Remaining Work

While the core functionality is working well, there are still opportunities for enhancement:

1. **Deeper Semantic Analysis**: Enhance symbol extraction by leveraging Ruff's semantic model for more accurate type information and reference tracking.

   For example, there's a bug with common variable names like `logger`:

   ```python
   # In target_module.py:
   logger = logging.getLogger(__name__)  # Defined in our target module

   # In external_file.py:
   # This file doesn't import target_module at all
   logger = logging.getLogger(__name__)  # Independently defined logger
   logger.info("Some message")  # This usage gets incorrectly counted as usage of target_module.logger
   ```

   The current implementation fails to verify that variables are actually imported from the target module before counting them as usage, causing false positives for common variable names.

2. **Advanced Import Resolution**: Improve handling of complex import patterns, especially for nested packages and module.symbol access patterns.

   For example, our current implementation struggles with this pattern:

   ```python
   # File structure:
   # src/
   #   pkg1/
   #     __init__.py
   #     pkg2/
   #       __init__.py  # contains: def some_function(), class SampleClass, SOME_CONSTANT

   # In client.py:
   from src.pkg1 import pkg2

   # These module.symbol access patterns aren't properly detected:
   result = pkg2.some_function()
   instance = pkg2.SampleClass()
   value = pkg2.SOME_CONSTANT
   ```

   The algorithm tracks direct imports like `from pkg2 import some_function` correctly, but struggles with nested package imports where the imported module is then used to access its attributes.

3. **Enhanced Usage Analysis**: Provide more detailed insights about how API symbols are used (e.g., read vs write access).
4. **Additional Features**:
   - Filtering options for public API reports
   - Dependency graphs between API components
   - API documentation template generation

## Leveraging Ruff's Infrastructure

This feature heavily relies on Ruff's existing capabilities:

- **Parsing:** `ruff_python_parser` for generating Abstract Syntax Trees (ASTs).
- **Semantic Analysis:** `ruff_python_semantic` for resolving bindings, references, and scopes.
- **Configuration & File Discovery:** `ruff_workspace` and `ruff_linter::resolver` to find relevant Python files, respect user configurations (exclusions, source roots), etc.

## Conceptual Data Structures (Rust)

```rust
// Represents a symbol defined within the target module/package
struct DefinedSymbol {
    name: String,
    kind: SymbolKind, // e.g., Function, Class, Variable
    definition_location: Location, // Start/end line/col in the target file
    docstring: Option<String>,
}

// The final result structure
struct UsageBasedApiResult {
    // Symbols DEFINED in the target AND USED externally at least once.
    public_api: HashMap<String, DefinedSymbol>,
    // Usage counts ONLY for symbols deemed public by usage.
    external_usage_counts: HashMap<String, usize>,
    // Optional: Could add symbols defined but never used externally.
    // unused_symbols: HashMap<String, DefinedSymbol>,
}

// Conceptual analysis function signature
fn analyze_api_by_usage(
    target_path: SystemPathBuf, // Path to the .py file or package directory
    workspace: &Workspace, // Ruff's workspace structure
    resolver: &Resolver, // Ruff's settings resolver
    all_project_files: &[ResolvedFile], // All Python files in the project context
) -> Result<UsageBasedApiResult>;
```

## Detailed Steps

1.  **Target Identification:**

    - Determine if the input `target_path` points to a `.py` file (module) or a directory (package).
    - Establish the "Target Boundary":
      - For a module file: The boundary is the single file path.
      - For a package directory: The boundary includes the `__init__.py` file AND all files/subdirectories within the package directory path.

2.  **Candidate Symbol Extraction (Target):**

    - Parse the target module file or the package's `__init__.py` file (and potentially other files within the package boundary, depending on desired depth).
    - Create a `SemanticModel` for each relevant file and analyze their top-level scopes.
    - Extract candidate symbols (`name`, `kind`, `location`, `docstring`) by iterating through the bindings in the global scope of each analyzed file.
    - Focus on bindings with kinds like `BindingKind::Name`, `BindingKind::Function`, `BindingKind::Class`, etc.
    - Store these as potential candidates (`candidate_symbols: HashMap<String, DefinedSymbol>`).
    - Initialize a temporary usage count map (`temp_usage_counts: HashMap<String, usize>`) for these candidates, starting counts at 0.

3.  **External Usage Analysis (Iterating over Project Files):**

    - Iterate through `all_project_files` provided for the project context.
    - **Filter for External Files:** For each file, check if its path is _outside_ the Target Boundary established in Step 1. Skip files that are _inside_ the boundary.
    - **Analyze Each External File:**
      - Create a `SemanticModel` for the external file.
      - Identify import statements related to the target module/package.
      - Use `resolve_qualified_name` to determine which imports resolve to symbols from the target boundary.
      - For each usage (reference) in the file, check if it corresponds to a symbol imported from the target boundary.
      - If a usage is found that corresponds to a candidate symbol from the target module/package, increment the counter for that symbol in the `temp_usage_counts` map.

4.  **Final API Construction:**

    - Create the final result maps: `public_api: HashMap<String, DefinedSymbol>` and `external_usage_counts: HashMap<String, usize>`.
    - Iterate through the `candidate_symbols` collected in Step 2.
    - For each candidate symbol (`name`, `defined_symbol`):
      - Check its count in `temp_usage_counts` from Step 3.
      - If the count is greater than 0:
        - Add the symbol's details to `public_api`.
        - Add its usage count to `external_usage_counts`.
      - _(Optional: Handle symbols with 0 count, e.g., adding them to an `unused_symbols` list/map)._
    - Assemble the `public_api` and `external_usage_counts` maps into the `UsageBasedApiResult` structure.

5.  **Integration into Ruff:**
    - Implement this logic within a suitable Rust module in the Ruff project (e.g., `ruff_analyze` or a new dedicated crate).
    - Create a new command (`ruff analyze api`) that:
      - Parses command-line arguments.
      - Sets up the workspace, semantic analysis infrastructure (`ruff_python_semantic`), and discovers project files.
      - Calls the core analysis function (`analyze_api_by_usage` or similar).
      - Formats the `UsageBasedApiResult` (e.g., as JSON) and prints it to output.

## Key Differences from Convention-Based Analysis

- The definition of "public" is entirely driven by observed external usage, not developer intent signaled via `__all__` or `_` prefixes.
- Requires analyzing the _entire project context_ to find usages, not just the target file.
- Symbols are first collected as candidates, then filtered based on usage counts.
