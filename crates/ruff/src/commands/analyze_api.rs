use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use log::debug;
use ruff_python_ast as ast;
use ruff_python_ast::ExprContext;
use ruff_workspace::resolver::{python_files_in_path, ResolvedFile, Resolver};
use serde::Serialize;

use crate::args::{AnalyzeApiArgs, ConfigArguments};
use crate::resolve;
use crate::{resolve_default_files, ExitStatus};

/// Symbol kinds we can detect and report on
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SymbolKind {
    Function,
    Class,
    Variable,
    Module,
    Other,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SymbolKind::Function => write!(f, "function"),
            SymbolKind::Class => write!(f, "class"),
            SymbolKind::Variable => write!(f, "variable"),
            SymbolKind::Module => write!(f, "module"),
            SymbolKind::Other => write!(f, "other"),
        }
    }
}

/// Information about a symbol defined in the target module/package
#[derive(Debug, Clone)]
struct DefinedSymbol {
    kind: SymbolKind,
    location: PathBuf,
    docstring: Option<String>,
    is_public: bool,              // Based on naming convention and __all__
    fully_qualified_name: String, // Added fully qualified name
}

/// Information about a symbol's usage in the codebase
#[derive(Debug)]
struct ApiSymbol {
    name: String,
    definition: DefinedSymbol,
    usage_count: usize,
    importers: HashSet<PathBuf>,
}

/// Analyze a Python module or package to determine its effective public API.
pub(crate) fn analyze_api(
    args: AnalyzeApiArgs,
    config_arguments: &ConfigArguments,
) -> Result<ExitStatus> {
    // Resolve project configuration
    let pyproject_config = resolve::resolve(config_arguments, None)?;
    let _settings = &pyproject_config.settings;

    println!("Analyzing API for: {}", args.target_path.display());

    // Check if the target path exists and is accessible
    if !args.target_path.exists() {
        anyhow::bail!("Target path does not exist: {}", args.target_path.display());
    }

    // Determine target boundary
    let target_boundary = determine_target_boundary(&args.target_path)?;

    // Find all Python files in the project (including the target)
    let project_root = detect_project_root(&args.target_path)?;

    let files = resolve_default_files(vec![project_root.clone()], false);
    let (paths, resolver) = python_files_in_path(&files, &pyproject_config, config_arguments)?;

    if paths.is_empty() {
        println!("No Python files found in the project");
        return Ok(ExitStatus::Success);
    }

    // Collect all project Python files and divide them into target and external files
    let mut target_files = Vec::new();
    let mut external_files = Vec::new();

    for resolved_result in paths {
        if let Ok(resolved_file) = resolved_result {
            let path = resolved_file.path().to_path_buf();

            // Determine if this file is within the target boundary
            if is_file_within_target(&path, &target_boundary) {
                target_files.push((path.clone(), resolved_file));
            } else {
                external_files.push((path.clone(), resolved_file));
            }
        }
    }

    println!(
        "Found {} target files and {} external files",
        target_files.len(),
        external_files.len()
    );

    if target_files.is_empty() {
        println!("No Python files found in the target path");
        return Ok(ExitStatus::Success);
    }

    // Extract candidate symbols from target files
    let candidate_symbols = extract_candidate_symbols(&target_files, &resolver)?;

    debug!(
        "Found {} candidate symbols in target",
        candidate_symbols.len()
    );

    // Analyze usage of target symbols in external files
    let public_api = analyze_external_usage(&candidate_symbols, &external_files, &resolver)?;

    // Output the results
    output_results(&public_api, &args)?;

    Ok(ExitStatus::Success)
}

/// Determine the project root by looking for pyproject.toml or package markers
fn detect_project_root(target_path: &PathBuf) -> Result<PathBuf> {
    let mut current_dir = if target_path.is_file() {
        target_path.parent().map(|p| p.to_path_buf())
    } else {
        Some(target_path.clone())
    };

    // Walk up the directory tree looking for project markers
    while let Some(dir) = current_dir {
        // Check for pyproject.toml
        let pyproject_path = dir.join("pyproject.toml");
        if pyproject_path.exists() {
            return Ok(dir);
        }

        // Check for setup.py or setup.cfg (common Python project markers)
        let setup_py_path = dir.join("setup.py");
        let setup_cfg_path = dir.join("setup.cfg");
        if setup_py_path.exists() || setup_cfg_path.exists() {
            return Ok(dir);
        }

        // Check for src directory with __init__.py (common Python project structure)
        let src_init_path = dir.join("src").join("__init__.py");
        if src_init_path.exists() {
            return Ok(dir);
        }

        // Move up to parent directory
        current_dir = dir.parent().map(|p| p.to_path_buf());
    }

    // If no project markers found, default to the parent directory of the target
    Ok(if target_path.is_file() {
        target_path.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else {
        target_path.clone()
    })
}

/// Determine the boundary of the target module/package
fn determine_target_boundary(target_path: &PathBuf) -> Result<Vec<PathBuf>> {
    let mut boundary = Vec::new();

    if target_path.is_file() {
        // For a single file, the boundary is just that file
        boundary.push(target_path.clone());
    } else if target_path.is_dir() {
        // For a package, include all Python files in the directory and subdirectories
        let walker = walkdir::WalkDir::new(target_path)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| {
                !e.file_name()
                    .to_str()
                    .map(|s| s.starts_with('.') || s == "__pycache__")
                    .unwrap_or(false)
            });

        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path().to_path_buf();
            if path.extension().map_or(false, |ext| ext == "py") {
                boundary.push(path);
            }
        }
    }

    Ok(boundary)
}

/// Check if a file is within the target boundary
fn is_file_within_target(file_path: &PathBuf, target_boundary: &[PathBuf]) -> bool {
    // For a module (single file), check exact match
    if target_boundary.len() == 1 && target_boundary[0].is_file() {
        return file_path == &target_boundary[0];
    }

    // For a package, check if file is in any subdirectory of the target
    for target_file in target_boundary {
        if file_path == target_file {
            return true;
        }
    }

    false
}

/// Extract candidate symbols from the target files using SemanticModel
fn extract_candidate_symbols(
    target_files: &[(PathBuf, ResolvedFile)],
    _resolver: &Resolver,
) -> Result<HashMap<String, DefinedSymbol>> {
    let mut candidates = HashMap::new();
    let _typing_modules: Vec<String> = Vec::new(); // Empty list for typing modules

    for (path, resolved_file) in target_files {
        // Read and parse the file content
        let file_content = std::fs::read_to_string(resolved_file.path())?;
        let parsed = ruff_python_parser::parse_module(&file_content);

        if let Ok(parsed) = parsed {
            // Get module name from the file path for qualified names
            let module_name = get_module_name_from_path(path);

            // Process the top-level names
            for stmt in &parsed.syntax().body {
                match stmt {
                    ast::Stmt::ClassDef(class_def) => {
                        // Process class definition
                        let name = class_def.name.as_str();
                        let is_private = name.starts_with('_')
                            && !name.starts_with("__")
                            && !name.ends_with("__");
                        let docstring = extract_docstring_from_body(&class_def.body);
                        let fully_qualified_name = format!("{}.{}", module_name, name);

                        candidates.insert(
                            name.to_string(),
                            DefinedSymbol {
                                kind: SymbolKind::Class,
                                location: path.clone(),
                                docstring,
                                is_public: !is_private,
                                fully_qualified_name,
                            },
                        );
                    }
                    ast::Stmt::FunctionDef(func_def) => {
                        // Process function definition
                        let name = func_def.name.as_str();
                        let is_private = name.starts_with('_')
                            && !name.starts_with("__")
                            && !name.ends_with("__");
                        let docstring = extract_docstring_from_body(&func_def.body);
                        let fully_qualified_name = format!("{}.{}", module_name, name);

                        candidates.insert(
                            name.to_string(),
                            DefinedSymbol {
                                kind: SymbolKind::Function,
                                location: path.clone(),
                                docstring,
                                is_public: !is_private,
                                fully_qualified_name,
                            },
                        );
                    }
                    ast::Stmt::Assign(assign) => {
                        // Process variable assignments
                        for target in &assign.targets {
                            if let ast::Expr::Name(name) = target {
                                let id = name.id.as_str();
                                let is_private = id.starts_with('_')
                                    && !id.starts_with("__")
                                    && !id.ends_with("__");
                                let fully_qualified_name = format!("{}.{}", module_name, id);

                                // Check if this is an __all__ definition
                                if id == "__all__" {
                                    if let ast::Expr::List(list) = &assign.value.as_ref() {
                                        for elt in &list.elts {
                                            if let ast::Expr::StringLiteral(string_lit) = elt {
                                                let value = string_lit.value.to_str();
                                                // Mark items in __all__ as public
                                                if let Some(symbol) = candidates.get_mut(value) {
                                                    symbol.is_public = true;
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    candidates.insert(
                                        id.to_string(),
                                        DefinedSymbol {
                                            kind: SymbolKind::Variable,
                                            location: path.clone(),
                                            docstring: None,
                                            is_public: !is_private,
                                            fully_qualified_name,
                                        },
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(candidates)
}

/// Helper function to extract a module name from a file path
fn get_module_name_from_path(path: &PathBuf) -> String {
    // Get the canonical path if possible to avoid relative path issues
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());

    // Extract filename without extension
    let file_stem = canonical_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    if file_stem == "__init__" {
        // For __init__.py files, use the directory structure
        if let Some(parent) = canonical_path.parent() {
            // Attempt to get package path components
            let package_components = get_package_components(parent);
            if !package_components.is_empty() {
                return package_components.join(".");
            }
        }
    } else {
        // For regular .py files, include parent package if in a package
        if let Some(parent) = canonical_path.parent() {
            let package_components = get_package_components(parent);
            if !package_components.is_empty() {
                let mut full_name = package_components;
                full_name.push(file_stem.to_string());
                return full_name.join(".");
            }
        }
    }

    // Default case: just return the file stem
    file_stem.to_string()
}

/// Helper function to walk up directory tree and collect package components
fn get_package_components(start_dir: &Path) -> Vec<String> {
    let mut components = Vec::new();
    let mut current_dir = Some(start_dir);

    // Walk up from the file toward root, collecting package names
    while let Some(dir) = current_dir {
        // Check if this directory is a Python package (has __init__.py)
        let has_init = dir.join("__init__.py").exists();

        if has_init {
            // If it's a package, add its name to components
            if let Some(dir_name) = dir.file_name() {
                if let Some(name) = dir_name.to_str() {
                    // Add to the beginning since we're walking backwards
                    components.insert(0, name.to_string());
                }
            }

            // Continue with parent
            current_dir = dir.parent();
        } else {
            // Stop when we reach a non-package directory
            break;
        }
    }

    components
}

/// Extract docstring from a body of statements
fn extract_docstring_from_body(body: &[ast::Stmt]) -> Option<String> {
    if !body.is_empty() {
        if let ast::Stmt::Expr(expr_stmt) = &body[0] {
            if let ast::Expr::StringLiteral(string_lit) = &expr_stmt.value.as_ref() {
                return Some(string_lit.value.to_string());
            }
        }
    }
    None
}

/// Analyze usage of target symbols in external files
fn analyze_external_usage(
    candidates: &HashMap<String, DefinedSymbol>,
    external_files: &[(PathBuf, ResolvedFile)],
    _resolver: &Resolver,
) -> Result<Vec<ApiSymbol>> {
    // Track usage of our public API symbols
    let mut usage_counts: HashMap<String, (usize, HashSet<PathBuf>)> = candidates
        .iter()
        .map(|(name, _)| (name.clone(), (0, HashSet::new())))
        .collect();

    // Keep track of processed symbols in each file to avoid double counting
    let mut processed_symbols: HashMap<PathBuf, HashSet<String>> = HashMap::new();

    // Track module imports and their aliases in each file
    let mut module_imports: HashMap<PathBuf, HashMap<String, String>> = HashMap::new();

    // Track which symbols were explicitly imported from our target in each file
    let mut imported_symbols: HashMap<PathBuf, HashSet<String>> = HashMap::new();

    // Determine the target module name from the first definition's location
    let target_module_name = if let Some((_, def)) = candidates.iter().next() {
        let file_path = &def.location;
        let file_stem = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        if file_stem == "__init__" {
            if let Some(parent) = file_path.parent() {
                let parent_name = parent
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                parent_name.to_string()
            } else {
                file_stem.to_string()
            }
        } else {
            file_stem.to_string()
        }
    } else {
        "unknown".to_string()
    };

    // Analyze each external file for imports and usage
    for (path, resolved_file) in external_files {
        // Read and parse the file content
        let file_content = std::fs::read_to_string(resolved_file.path())?;
        let parsed = ruff_python_parser::parse_module(&file_content);

        if let Ok(parsed) = parsed {
            // Get or create the set of processed symbols for this file
            let file_processed = processed_symbols
                .entry(path.clone())
                .or_insert_with(HashSet::new);

            // Get or create module imports mapping for this file
            let file_modules = module_imports
                .entry(path.clone())
                .or_insert_with(HashMap::new);

            // Get or create imported symbols set for this file
            let file_imported = imported_symbols
                .entry(path.clone())
                .or_insert_with(HashSet::new);

            // Process imports
            for stmt in &parsed.syntax().body {
                match stmt {
                    ast::Stmt::Import(import) => {
                        // Handle direct imports
                        for alias in &import.names {
                            let module_name = alias.name.as_str();

                            // Track module imports and their aliases
                            if let Some(asname) = &alias.asname {
                                file_modules.insert(asname.to_string(), module_name.to_string());
                            } else {
                                file_modules
                                    .insert(module_name.to_string(), module_name.to_string());
                            }

                            // Identify the module name without path
                            let simple_module_name =
                                module_name.split('.').next().unwrap_or(module_name);

                            // Check if this module being imported is our target module
                            if simple_module_name == target_module_name {
                                // Mark the module itself as imported from our target
                                file_imported.insert(module_name.to_string());
                            }

                            // Check if the module is one of our candidate symbols
                            if candidates.contains_key(simple_module_name)
                                && !file_processed.contains(simple_module_name)
                            {
                                if let Some(entry) = usage_counts.get_mut(simple_module_name) {
                                    entry.0 += 1;
                                    entry.1.insert(path.clone());
                                    file_processed.insert(simple_module_name.to_string());
                                    // Track this symbol as being imported from our target
                                    file_imported.insert(module_name.to_string());
                                }
                            }
                        }
                    }
                    ast::Stmt::ImportFrom(import_from) => {
                        // Handle from-imports
                        if let Some(module_name) = &import_from.module {
                            // When importing a module with "from", track what was imported
                            let module_name_str = module_name.to_string();

                            // Check if this is an import from our target module
                            let simple_module_name = module_name_str
                                .split('.')
                                .next()
                                .unwrap_or(&module_name_str);
                            let is_target_module = simple_module_name == target_module_name;

                            for alias in &import_from.names {
                                let name = alias.name.as_str();

                                // Handle "from pkg1 import pkg2" case
                                if let Some(asname) = &alias.asname {
                                    file_modules.insert(asname.to_string(), name.to_string());
                                } else {
                                    file_modules.insert(name.to_string(), name.to_string());
                                }

                                // If this is an import from our target module, add it to the imported_symbols
                                if is_target_module {
                                    file_imported.insert(name.to_string());
                                }

                                // Check if the imported symbol is in our candidates
                                if candidates.contains_key(name) && !file_processed.contains(name) {
                                    if let Some(entry) = usage_counts.get_mut(name) {
                                        entry.0 += 1;
                                        entry.1.insert(path.clone());
                                        file_processed.insert(name.to_string());
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Scan for attribute accesses to find module.symbol patterns
            for stmt in &parsed.syntax().body {
                scan_stmt_for_attribute_access(
                    stmt,
                    &file_modules,
                    path,
                    file_processed,
                    file_imported,
                    candidates,
                    &mut usage_counts,
                    &target_module_name,
                );
            }
        }
    }

    // Convert usage counts to API symbols list
    let mut public_api = Vec::new();
    for (symbol_name, (count, importers)) in usage_counts {
        if count > 0 {
            if let Some(definition) = candidates.get(&symbol_name) {
                public_api.push(ApiSymbol {
                    name: symbol_name,
                    definition: definition.clone(),
                    usage_count: count,
                    importers,
                });
            }
        }
    }

    // Sort by name for consistent output
    public_api.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(public_api)
}

/// Helper function to scan statements for attribute access
fn scan_stmt_for_attribute_access(
    stmt: &ast::Stmt,
    module_imports: &HashMap<String, String>,
    file_path: &PathBuf,
    file_processed: &mut HashSet<String>,
    file_imported: &HashSet<String>,
    candidates: &HashMap<String, DefinedSymbol>,
    usage_counts: &mut HashMap<String, (usize, HashSet<PathBuf>)>,
    target_module_name: &str,
) {
    match stmt {
        ast::Stmt::Expr(expr_stmt) => {
            scan_expr_for_attribute_access(
                &expr_stmt.value,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
        }
        ast::Stmt::Assign(assign) => {
            scan_expr_for_attribute_access(
                &assign.value,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
            // Also scan targets
            for target in &assign.targets {
                scan_expr_for_attribute_access(
                    target,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
        }
        ast::Stmt::AugAssign(aug_assign) => {
            scan_expr_for_attribute_access(
                &aug_assign.value,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
            scan_expr_for_attribute_access(
                &aug_assign.target,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
        }
        ast::Stmt::AnnAssign(ann_assign) => {
            if let Some(value) = &ann_assign.value {
                scan_expr_for_attribute_access(
                    value,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
            scan_expr_for_attribute_access(
                &ann_assign.target,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
            scan_expr_for_attribute_access(
                &ann_assign.annotation,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
        }
        ast::Stmt::Return(ret) => {
            if let Some(value) = &ret.value {
                scan_expr_for_attribute_access(
                    value,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
        }
        ast::Stmt::If(if_stmt) => {
            scan_expr_for_attribute_access(
                &if_stmt.test,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
            for substmt in &if_stmt.body {
                scan_stmt_for_attribute_access(
                    substmt,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
            // Process elif/else clauses correctly
            for clause in &if_stmt.elif_else_clauses {
                if let Some(test) = &clause.test {
                    scan_expr_for_attribute_access(
                        test,
                        module_imports,
                        file_path,
                        file_processed,
                        file_imported,
                        candidates,
                        usage_counts,
                        target_module_name,
                    );
                }
                for substmt in &clause.body {
                    scan_stmt_for_attribute_access(
                        substmt,
                        module_imports,
                        file_path,
                        file_processed,
                        file_imported,
                        candidates,
                        usage_counts,
                        target_module_name,
                    );
                }
            }
        }
        ast::Stmt::For(for_stmt) => {
            scan_expr_for_attribute_access(
                &for_stmt.target,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
            scan_expr_for_attribute_access(
                &for_stmt.iter,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
            for substmt in &for_stmt.body {
                scan_stmt_for_attribute_access(
                    substmt,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
            for substmt in &for_stmt.orelse {
                scan_stmt_for_attribute_access(
                    substmt,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
        }
        ast::Stmt::While(while_stmt) => {
            scan_expr_for_attribute_access(
                &while_stmt.test,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
            for substmt in &while_stmt.body {
                scan_stmt_for_attribute_access(
                    substmt,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
            for substmt in &while_stmt.orelse {
                scan_stmt_for_attribute_access(
                    substmt,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
        }
        ast::Stmt::Assert(assert_stmt) => {
            scan_expr_for_attribute_access(
                &assert_stmt.test,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
            if let Some(msg) = &assert_stmt.msg {
                scan_expr_for_attribute_access(
                    msg,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
        }
        ast::Stmt::FunctionDef(func_def) => {
            // Scan function body
            for substmt in &func_def.body {
                scan_stmt_for_attribute_access(
                    substmt,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
            // Scan parameters
            if let Some(returns) = &func_def.returns {
                scan_expr_for_attribute_access(
                    returns,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }

            // Scan decorators
            for decorator in &func_def.decorator_list {
                scan_expr_for_attribute_access(
                    &decorator.expression,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
        }
        ast::Stmt::ClassDef(class_def) => {
            // Scan class body
            for substmt in &class_def.body {
                scan_stmt_for_attribute_access(
                    substmt,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }

            // Scan arguments if present
            if let Some(args) = &class_def.arguments {
                // Scan positional args
                for arg in &args.args {
                    scan_expr_for_attribute_access(
                        arg,
                        module_imports,
                        file_path,
                        file_processed,
                        file_imported,
                        candidates,
                        usage_counts,
                        target_module_name,
                    );
                }

                // Scan keyword args
                for keyword in &args.keywords {
                    scan_expr_for_attribute_access(
                        &keyword.value,
                        module_imports,
                        file_path,
                        file_processed,
                        file_imported,
                        candidates,
                        usage_counts,
                        target_module_name,
                    );
                }
            }

            // Scan decorators
            for decorator in &class_def.decorator_list {
                scan_expr_for_attribute_access(
                    &decorator.expression,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
        }
        // Add more statement types as needed
        _ => {}
    }
}

/// Helper function to scan expressions for attribute access
fn scan_expr_for_attribute_access(
    expr: &ast::Expr,
    module_imports: &HashMap<String, String>,
    file_path: &PathBuf,
    file_processed: &mut HashSet<String>,
    file_imported: &HashSet<String>,
    candidates: &HashMap<String, DefinedSymbol>,
    usage_counts: &mut HashMap<String, (usize, HashSet<PathBuf>)>,
    target_module_name: &str,
) {
    match expr {
        ast::Expr::Attribute(attr) => {
            // Check if this is a module.symbol pattern
            if let ast::Expr::Name(name) = &attr.value.as_ref() {
                let module_alias = name.id.as_str();

                // If this is a module we've imported
                if module_imports.contains_key(module_alias) {
                    let accessed_attr = attr.attr.as_str();

                    // Check if this symbol is in our candidates
                    if candidates.contains_key(accessed_attr)
                        && !file_processed.contains(accessed_attr)
                    {
                        // Check if the module is our target module or an alias to it
                        let is_target_module =
                            if let Some(actual_module) = module_imports.get(module_alias) {
                                let simple_module =
                                    actual_module.split('.').next().unwrap_or(actual_module);
                                simple_module == target_module_name
                            } else {
                                false
                            };

                        if is_target_module {
                            if let Some(entry) = usage_counts.get_mut(accessed_attr) {
                                entry.0 += 1;
                                entry.1.insert(file_path.clone());
                                file_processed.insert(accessed_attr.to_string());
                            }
                        }
                    }
                }
            }

            // Recursively scan the value expression
            scan_expr_for_attribute_access(
                &attr.value,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
        }
        ast::Expr::Name(name) => {
            // When we encounter a name, only count it if it was explicitly imported from our target
            let symbol_name = name.id.as_str();
            if candidates.contains_key(symbol_name)
                && file_imported.contains(symbol_name)
                && !file_processed.contains(symbol_name)
                && name.ctx == ExprContext::Load
            {
                if let Some(entry) = usage_counts.get_mut(symbol_name) {
                    entry.0 += 1;
                    entry.1.insert(file_path.clone());
                    file_processed.insert(symbol_name.to_string());
                }
            }
        }
        ast::Expr::Call(call) => {
            // Scan the function being called
            scan_expr_for_attribute_access(
                &call.func,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );

            // Scan arguments
            for arg in &call.arguments.args {
                scan_expr_for_attribute_access(
                    arg,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }

            // Scan keywords
            for keyword in &call.arguments.keywords {
                scan_expr_for_attribute_access(
                    &keyword.value,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
        }
        ast::Expr::BinOp(binop) => {
            scan_expr_for_attribute_access(
                &binop.left,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
            scan_expr_for_attribute_access(
                &binop.right,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
        }
        ast::Expr::UnaryOp(unaryop) => {
            scan_expr_for_attribute_access(
                &unaryop.operand,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
        }
        ast::Expr::Compare(compare) => {
            scan_expr_for_attribute_access(
                &compare.left,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
            for comp in &compare.comparators {
                scan_expr_for_attribute_access(
                    comp,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
        }
        ast::Expr::Subscript(subscript) => {
            scan_expr_for_attribute_access(
                &subscript.value,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
            scan_expr_for_attribute_access(
                &subscript.slice,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
        }
        ast::Expr::List(list) => {
            for elt in &list.elts {
                scan_expr_for_attribute_access(
                    elt,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
        }
        ast::Expr::Tuple(tuple) => {
            for elt in &tuple.elts {
                scan_expr_for_attribute_access(
                    elt,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
        }
        ast::Expr::Dict(dict) => {
            for item in &dict.items {
                if let Some(key) = &item.key {
                    scan_expr_for_attribute_access(
                        key,
                        module_imports,
                        file_path,
                        file_processed,
                        file_imported,
                        candidates,
                        usage_counts,
                        target_module_name,
                    );
                }
                scan_expr_for_attribute_access(
                    &item.value,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
        }
        ast::Expr::Set(set) => {
            for elt in &set.elts {
                scan_expr_for_attribute_access(
                    elt,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
            }
        }
        ast::Expr::Lambda(lambda) => {
            scan_expr_for_attribute_access(
                &lambda.body,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
        }
        ast::Expr::If(ifexp) => {
            scan_expr_for_attribute_access(
                &ifexp.test,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
            scan_expr_for_attribute_access(
                &ifexp.body,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
            scan_expr_for_attribute_access(
                &ifexp.orelse,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
        }
        ast::Expr::ListComp(listcomp) => {
            scan_expr_for_attribute_access(
                &listcomp.elt,
                module_imports,
                file_path,
                file_processed,
                file_imported,
                candidates,
                usage_counts,
                target_module_name,
            );
            for comp in &listcomp.generators {
                scan_expr_for_attribute_access(
                    &comp.target,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
                scan_expr_for_attribute_access(
                    &comp.iter,
                    module_imports,
                    file_path,
                    file_processed,
                    file_imported,
                    candidates,
                    usage_counts,
                    target_module_name,
                );
                for if_expr in &comp.ifs {
                    scan_expr_for_attribute_access(
                        if_expr,
                        module_imports,
                        file_path,
                        file_processed,
                        file_imported,
                        candidates,
                        usage_counts,
                        target_module_name,
                    );
                }
            }
        }
        // Handle other expression types as needed
        _ => {}
    }
}

/// Output the results of the API analysis
fn output_results(public_api: &[ApiSymbol], args: &AnalyzeApiArgs) -> Result<()> {
    use colored::Colorize;

    // Determine the output format
    let format = args.output_format.as_deref().unwrap_or("text");

    match format {
        "json" => {
            // Create a serializable structure for JSON output
            #[derive(Serialize)]
            struct JsonOutput {
                public_api: Vec<JsonApiSymbol>,
                target_path: String,
            }

            #[derive(Serialize)]
            struct JsonApiSymbol {
                name: String,
                fully_qualified_name: String,
                kind: String,
                location: String,
                docstring: Option<String>,
                usage_count: usize,
                importers: Vec<String>,
                is_public: bool,
            }

            // Convert our API symbols to the serializable format
            let api_json: Vec<JsonApiSymbol> = public_api
                .iter()
                .map(|sym| JsonApiSymbol {
                    name: sym.name.clone(),
                    fully_qualified_name: sym.definition.fully_qualified_name.clone(),
                    kind: sym.definition.kind.to_string(),
                    location: sym.definition.location.display().to_string(),
                    docstring: sym.definition.docstring.clone(),
                    usage_count: sym.usage_count,
                    importers: sym
                        .importers
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect(),
                    is_public: sym.definition.is_public,
                })
                .collect();

            // Create the final output structure
            let output = JsonOutput {
                public_api: api_json,
                target_path: args.target_path.display().to_string(),
            };

            // Serialize to JSON and print
            let json = serde_json::to_string_pretty(&output)?;
            println!("{}", json);
        }

        // Default to text output
        _ => {
            if public_api.is_empty() {
                println!("No public API symbols found with external usage.");
                return Ok(());
            }

            println!(
                "Public API for {}:",
                args.target_path.display().to_string().bold()
            );
            println!();

            // Group by kind for better organization
            let mut by_kind: HashMap<&SymbolKind, Vec<&ApiSymbol>> = HashMap::new();

            for symbol in public_api {
                by_kind
                    .entry(&symbol.definition.kind)
                    .or_default()
                    .push(symbol);
            }

            let kinds = [
                &SymbolKind::Class,
                &SymbolKind::Function,
                &SymbolKind::Variable,
                &SymbolKind::Module,
                &SymbolKind::Other,
            ];

            for kind in kinds.iter() {
                if let Some(symbols) = by_kind.get(kind) {
                    if !symbols.is_empty() {
                        // Print section header
                        println!("{}:", kind.to_string().to_uppercase().bold());

                        for symbol in symbols {
                            // Print symbol name and usage count with visibility indicator
                            let visibility = if symbol.definition.is_public {
                                "public".green()
                            } else {
                                "private".red()
                            };

                            println!(
                                "  {} ({} external usages, {})",
                                symbol.name.cyan().bold(),
                                symbol.usage_count.to_string().green(),
                                visibility
                            );

                            // Print fully qualified name
                            println!(
                                "    Fully qualified: {}",
                                symbol.definition.fully_qualified_name.cyan()
                            );

                            // Print docstring if available
                            if let Some(docstring) = &symbol.definition.docstring {
                                let docstring = docstring.trim_matches('"').trim();
                                if !docstring.is_empty() {
                                    println!("    {}", docstring.italic());
                                }
                            }

                            // Print location
                            println!(
                                "    Location: {}",
                                symbol.definition.location.display().to_string().dimmed()
                            );

                            // Print a list of up to 3 importers
                            let importer_count = symbol.importers.len();
                            if importer_count > 0 {
                                let sample: Vec<_> = symbol
                                    .importers
                                    .iter()
                                    .take(3)
                                    .map(|p| p.display().to_string())
                                    .collect();

                                if importer_count <= 3 {
                                    println!("    Imported by: {}", sample.join(", ").dimmed());
                                } else {
                                    println!(
                                        "    Imported by: {} and {} more files",
                                        sample.join(", ").dimmed(),
                                        (importer_count - 3).to_string().dimmed()
                                    );
                                }
                            }

                            println!();
                        }
                    }
                }
            }

            // Print summary
            println!(
                "Found {} public API symbols with external usage.",
                public_api.len().to_string().bold()
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::tempdir;

    #[test]
    fn test_detect_project_root_pyproject() -> Result<()> {
        let temp_dir = tempdir()?;
        let project_dir = temp_dir.path().join("my_project");
        fs::create_dir_all(&project_dir)?;

        // Create a pyproject.toml file
        let pyproject_path = project_dir.join("pyproject.toml");
        File::create(&pyproject_path)?;

        // Create a Python file
        let module_dir = project_dir.join("src");
        fs::create_dir_all(&module_dir)?;
        let module_path = module_dir.join("module.py");
        File::create(&module_path)?;

        // Test with target path as the module
        let root = detect_project_root(&module_path.to_path_buf())?;
        assert_eq!(root, project_dir);

        // Test with target path as the project dir
        let root = detect_project_root(&project_dir.to_path_buf())?;
        assert_eq!(root, project_dir);

        Ok(())
    }

    #[test]
    fn test_detect_project_root_setup_py() -> Result<()> {
        let temp_dir = tempdir()?;
        let project_dir = temp_dir.path().join("my_project");
        fs::create_dir_all(&project_dir)?;

        // Create a setup.py file
        let setup_path = project_dir.join("setup.py");
        File::create(&setup_path)?;

        // Create a Python file
        let module_dir = project_dir.join("src");
        fs::create_dir_all(&module_dir)?;
        let module_path = module_dir.join("module.py");
        File::create(&module_path)?;

        // Test with target path as the module
        let root = detect_project_root(&module_path.to_path_buf())?;
        assert_eq!(root, project_dir);

        Ok(())
    }

    #[test]
    fn test_detect_project_root_fallback() -> Result<()> {
        let temp_dir = tempdir()?;
        let project_dir = temp_dir.path().join("my_project");
        fs::create_dir_all(&project_dir)?;

        // Create a Python file with no project markers
        let module_path = project_dir.join("module.py");
        File::create(&module_path)?;

        // Test with target path as the module
        let root = detect_project_root(&module_path.to_path_buf())?;
        assert_eq!(root, project_dir); // Should resolve to parent directory

        Ok(())
    }
}
