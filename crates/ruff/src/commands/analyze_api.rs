use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use log::{debug, info, trace};
use rayon::prelude::*;
use ruff_python_ast as ast;
use ruff_python_ast::ExprContext;
use ruff_workspace::resolver::{python_files_in_path, ResolvedFile, Resolver};
use serde::Serialize;
use walkdir::WalkDir;

use crate::args::{AnalyzeApiArgs, ConfigArguments};
use crate::resolve;
use crate::{resolve_default_files, ExitStatus};

//------------------------------------------------------------------------------
// Type aliases for complex types
//------------------------------------------------------------------------------

/// Maps a symbol name to its usage count and importing files
type SymbolUsageMap = HashMap<String, (usize, HashSet<PathBuf>)>;

/// A collection of Python files with their resolved information
type ResolvedFileCollection = Vec<(PathBuf, ResolvedFile)>;

//------------------------------------------------------------------------------
// Analysis context structs
//------------------------------------------------------------------------------

/// Struct to hold per-file analysis state
pub(crate) struct FileAnalysisState {
    /// Symbols already processed in this file to avoid duplicates
    processed_symbols: HashSet<String>,

    /// Map from module alias to actual module name
    module_aliases: HashMap<String, String>,

    /// Symbols imported from the target module
    imported_symbols: HashSet<String>,
}

impl FileAnalysisState {
    /// Create a new file analysis state
    pub(crate) fn new() -> Self {
        Self {
            processed_symbols: HashSet::new(),
            module_aliases: HashMap::new(),
            imported_symbols: HashSet::new(),
        }
    }

    /// Check if a symbol has been processed
    pub(crate) fn is_processed(&self, symbol: &str) -> bool {
        self.processed_symbols.contains(symbol)
    }

    /// Mark a symbol as processed
    pub(crate) fn mark_processed(&mut self, symbol: String) {
        self.processed_symbols.insert(symbol);
    }

    /// Register a module alias
    pub(crate) fn register_module_alias(&mut self, alias: String, actual_name: String) {
        self.module_aliases.insert(alias, actual_name);
    }

    /// Get the actual module name for an alias
    pub(crate) fn get_actual_module_name(&self, alias: &str) -> Option<&String> {
        self.module_aliases.get(alias)
    }

    /// Register a symbol as imported from the target module
    pub(crate) fn register_imported_symbol(&mut self, symbol: String) {
        self.imported_symbols.insert(symbol);
    }

    /// Check if a symbol was imported from the target module
    pub(crate) fn is_imported_from_target(&self, symbol: &str) -> bool {
        self.imported_symbols.contains(symbol)
    }
}

/// Struct to hold the overall analysis state
pub(crate) struct ApiAnalyzer {
    /// Candidate symbols to check for usage
    candidates: HashMap<String, DefinedSymbol>,

    /// Usage counts for candidate symbols
    usage_counts: Arc<Mutex<SymbolUsageMap>>,

    /// Name of the target module being analyzed
    target_module_name: String,
}

impl ApiAnalyzer {
    /// Create a new API analyzer
    pub(crate) fn new(
        candidates: HashMap<String, DefinedSymbol>,
        target_module_name: String,
    ) -> Self {
        let usage_counts = Arc::new(Mutex::new(
            candidates
                .iter()
                .map(|(name, _)| (name.clone(), (0, HashSet::new())))
                .collect::<SymbolUsageMap>(),
        ));

        Self {
            candidates,
            usage_counts,
            target_module_name,
        }
    }

    /// Check if a symbol is in the candidates list
    pub(crate) fn is_candidate_symbol(&self, symbol: &str) -> bool {
        self.candidates.contains_key(symbol)
    }

    /// Record usage of a symbol in a file
    pub(crate) fn record_symbol_usage(&self, symbol: &str, file_path: &Path) -> Result<()> {
        let mut usage_map = self
            .usage_counts
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to acquire lock on usage counts"))?;

        if let Some(entry) = usage_map.get_mut(symbol) {
            entry.0 += 1;
            entry.1.insert(file_path.to_path_buf());
        }

        Ok(())
    }

    /// Build the final list of API symbols
    pub(crate) fn build_api_symbols(&self) -> Result<Vec<ApiSymbol>> {
        // Convert usage data to API symbols list
        let final_usage_counts = match Arc::try_unwrap(self.usage_counts.clone()) {
            Ok(mutex) => mutex.into_inner()?,
            Err(arc) => arc
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to acquire lock"))?
                .clone(),
        };

        // Convert usage counts to API symbols list
        let mut public_api = Vec::new();

        for (symbol_name, (count, importers)) in final_usage_counts {
            if count > 0 {
                if let Some(definition) = self.candidates.get(&symbol_name) {
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
}

//------------------------------------------------------------------------------
// Symbol and API data structures
//------------------------------------------------------------------------------

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
            Self::Function => write!(f, "function"),
            Self::Class => write!(f, "class"),
            Self::Variable => write!(f, "variable"),
            Self::Module => write!(f, "module"),
            Self::Other => write!(f, "other"),
        }
    }
}

/// Information about a symbol defined in the target module/package
#[derive(Debug, Clone)]
pub(crate) struct DefinedSymbol {
    kind: SymbolKind,
    location: PathBuf,
    docstring: Option<String>,
    is_public: bool,              // Based on naming convention and __all__
    fully_qualified_name: String, // Complete import path for the symbol
}

/// Information about a symbol's usage in the codebase
#[derive(Debug, Clone)]
pub(crate) struct ApiSymbol {
    name: String,
    definition: DefinedSymbol,
    usage_count: usize,
    importers: HashSet<PathBuf>, // Files that import and use this symbol
}

//------------------------------------------------------------------------------
// AST Visitor implementation for API analysis
//------------------------------------------------------------------------------

/// Trait defining a visitor for Python AST traversal
pub(crate) trait AstVisitor {
    /// Visit a statement node
    fn visit_stmt(&mut self, stmt: &ast::Stmt);

    /// Visit an expression node
    fn visit_expr(&mut self, expr: &ast::Expr);
}

/// Implementation of the Visitor pattern for API analysis
pub(crate) struct ApiAnalyzerVisitor<'a> {
    /// Current file being processed
    file_path: &'a Path,

    /// Reference to the analyzer with shared state
    analyzer: &'a ApiAnalyzer,

    /// File-specific state for the current file
    file_state: &'a mut FileAnalysisState,
}

impl<'a> ApiAnalyzerVisitor<'a> {
    /// Create a new visitor instance
    pub(crate) fn new(
        file_path: &'a Path,
        analyzer: &'a ApiAnalyzer,
        file_state: &'a mut FileAnalysisState,
    ) -> Self {
        Self {
            file_path,
            analyzer,
            file_state,
        }
    }

    /// Process an import statement to track module imports and their aliases
    pub(crate) fn process_imports(&mut self, statements: &[ast::Stmt]) {
        for stmt in statements {
            match stmt {
                ast::Stmt::Import(import) => {
                    // Handle direct imports
                    for alias in &import.names {
                        let module_name = alias.name.as_str();

                        // Track module imports and their aliases
                        if let Some(asname) = &alias.asname {
                            self.file_state
                                .register_module_alias(asname.to_string(), module_name.to_string());
                        } else {
                            self.file_state.register_module_alias(
                                module_name.to_string(),
                                module_name.to_string(),
                            );
                        }

                        // Identify the module name without path
                        let simple_module_name =
                            module_name.split('.').next().unwrap_or(module_name);

                        // Check if this module being imported is our target module
                        if simple_module_name == self.analyzer.target_module_name {
                            // Mark the module itself as imported from our target
                            self.file_state
                                .register_imported_symbol(module_name.to_string());
                        }

                        // Check if the module is one of our candidate symbols
                        if self.analyzer.is_candidate_symbol(simple_module_name)
                            && !self.file_state.is_processed(simple_module_name)
                        {
                            if let Err(e) = self
                                .analyzer
                                .record_symbol_usage(simple_module_name, self.file_path)
                            {
                                debug!("Error recording symbol usage: {}", e);
                            }
                            self.file_state
                                .mark_processed(simple_module_name.to_string());
                            // Track this symbol as being imported from our target
                            self.file_state
                                .register_imported_symbol(module_name.to_string());
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
                        let is_target_module =
                            simple_module_name == self.analyzer.target_module_name;

                        for alias in &import_from.names {
                            let name = alias.name.as_str();

                            // Handle "from pkg1 import pkg2" case
                            if let Some(asname) = &alias.asname {
                                self.file_state
                                    .register_module_alias(asname.to_string(), name.to_string());
                            } else {
                                self.file_state
                                    .register_module_alias(name.to_string(), name.to_string());
                            }

                            // If this is an import from our target module, add it to the imported_symbols
                            if is_target_module {
                                self.file_state.register_imported_symbol(name.to_string());
                            }

                            // Check if the imported symbol is in our candidates by comparing both base name and fully qualified name
                            if self.analyzer.is_candidate_symbol(name)
                                && !self.file_state.is_processed(name)
                            {
                                // Construct the expected fully qualified name directly
                                let mut expected_fqn = module_name_str.clone();
                                expected_fqn.push('.');
                                expected_fqn.push_str(name);

                                // Get the candidate symbol
                                let matching = self
                                    .analyzer
                                    .candidates
                                    .get(name)
                                    .map(|sym| {
                                        // Only consider it a match if the fully qualified name matches or starts with the expected FQN
                                        sym.fully_qualified_name == expected_fqn
                                            || expected_fqn.starts_with(&sym.fully_qualified_name)
                                            || sym.fully_qualified_name.ends_with(&expected_fqn)
                                    })
                                    .unwrap_or(false);

                                // Only count usage if the fully qualified name matches
                                if matching {
                                    if let Err(e) =
                                        self.analyzer.record_symbol_usage(name, self.file_path)
                                    {
                                        debug!("Error recording symbol usage: {}", e);
                                    }
                                    self.file_state.mark_processed(name.to_string());
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Record usage of a symbol
    fn record_symbol_usage(&self, symbol: &str) {
        if self.analyzer.is_candidate_symbol(symbol)
            && self.file_state.is_imported_from_target(symbol)
            && !self.file_state.is_processed(symbol)
        {
            if let Err(e) = self.analyzer.record_symbol_usage(symbol, self.file_path) {
                debug!("Error recording symbol usage: {}", e);
            }
        }
    }

    /// Check for module.symbol pattern and record if found
    fn check_attribute_access(&self, attr: &ast::ExprAttribute) {
        if let ast::Expr::Name(name) = &attr.value.as_ref() {
            let module_alias = name.id.as_str();

            // If this is a module we've imported
            if let Some(actual_module_name) = self.file_state.get_actual_module_name(module_alias) {
                let accessed_attr = attr.attr.as_str();

                // Check if this symbol is in our candidates
                if self.analyzer.is_candidate_symbol(accessed_attr)
                    && !self.file_state.is_processed(accessed_attr)
                {
                    // Avoid redundant format! calls by constructing the expected FQN directly
                    let mut expected_fqn = actual_module_name.to_owned();
                    expected_fqn.push('.');
                    expected_fqn.push_str(accessed_attr);

                    // Get the candidate symbol and check if its fully qualified name matches
                    let matching = self
                        .analyzer
                        .candidates
                        .get(accessed_attr)
                        .map(|sym| {
                            // Only consider it a match if the fully qualified name matches or ends with the expected FQN
                            sym.fully_qualified_name == expected_fqn
                                || expected_fqn.starts_with(&sym.fully_qualified_name)
                                || sym.fully_qualified_name.ends_with(&expected_fqn)
                        })
                        .unwrap_or(false);

                    // Check if the module is our target module or an alias to it
                    let simple_module = actual_module_name
                        .split('.')
                        .next()
                        .unwrap_or(actual_module_name);
                    let is_target_module = simple_module == self.analyzer.target_module_name;

                    if is_target_module && matching {
                        if let Err(e) = self
                            .analyzer
                            .record_symbol_usage(accessed_attr, self.file_path)
                        {
                            debug!("Error recording symbol usage: {}", e);
                        }
                    }
                }
            }
        }
    }
}

impl<'a> AstVisitor for ApiAnalyzerVisitor<'a> {
    fn visit_stmt(&mut self, stmt: &ast::Stmt) {
        match stmt {
            // Expression statement (standalone expression)
            ast::Stmt::Expr(expr_stmt) => {
                self.visit_expr(&expr_stmt.value);
            }

            // Assignment statement
            ast::Stmt::Assign(assign) => {
                self.visit_expr(&assign.value);
                for target in &assign.targets {
                    self.visit_expr(target);
                }
            }

            // Augmented assignment (e.g., x += 1)
            ast::Stmt::AugAssign(aug_assign) => {
                self.visit_expr(&aug_assign.value);
                self.visit_expr(&aug_assign.target);
            }

            // Annotated assignment (e.g., x: int = 1)
            ast::Stmt::AnnAssign(ann_assign) => {
                if let Some(value) = &ann_assign.value {
                    self.visit_expr(value);
                }
                self.visit_expr(&ann_assign.target);
                self.visit_expr(&ann_assign.annotation);
            }

            // Return statement
            ast::Stmt::Return(ret) => {
                if let Some(value) = &ret.value {
                    self.visit_expr(value);
                }
            }

            // If statement
            ast::Stmt::If(if_stmt) => {
                self.visit_expr(&if_stmt.test);

                // Scan the if-body
                for substmt in &if_stmt.body {
                    self.visit_stmt(substmt);
                }

                // Scan the elif/else clauses
                for clause in &if_stmt.elif_else_clauses {
                    if let Some(test) = &clause.test {
                        self.visit_expr(test);
                    }
                    for substmt in &clause.body {
                        self.visit_stmt(substmt);
                    }
                }
            }

            // For loop
            ast::Stmt::For(for_stmt) => {
                self.visit_expr(&for_stmt.target);
                self.visit_expr(&for_stmt.iter);

                // Scan the loop body
                for substmt in &for_stmt.body {
                    self.visit_stmt(substmt);
                }

                // Scan the else clause
                for substmt in &for_stmt.orelse {
                    self.visit_stmt(substmt);
                }
            }

            // While loop
            ast::Stmt::While(while_stmt) => {
                self.visit_expr(&while_stmt.test);

                // Scan the loop body
                for substmt in &while_stmt.body {
                    self.visit_stmt(substmt);
                }

                // Scan the else clause
                for substmt in &while_stmt.orelse {
                    self.visit_stmt(substmt);
                }
            }

            // Assert statement
            ast::Stmt::Assert(assert_stmt) => {
                self.visit_expr(&assert_stmt.test);
                if let Some(msg) = &assert_stmt.msg {
                    self.visit_expr(msg);
                }
            }

            // Function definition
            ast::Stmt::FunctionDef(func_def) => {
                // Scan function body
                for substmt in &func_def.body {
                    self.visit_stmt(substmt);
                }

                // Scan return type annotation
                if let Some(returns) = &func_def.returns {
                    self.visit_expr(returns);
                }

                // Scan decorators
                for decorator in &func_def.decorator_list {
                    self.visit_expr(&decorator.expression);
                }
            }

            // Class definition
            ast::Stmt::ClassDef(class_def) => {
                // Scan class body
                for substmt in &class_def.body {
                    self.visit_stmt(substmt);
                }

                // Scan base classes and metaclasses
                if let Some(args) = &class_def.arguments {
                    // Scan positional args (base classes)
                    for arg in &args.args {
                        self.visit_expr(arg);
                    }

                    // Scan keyword args (metaclass, etc.)
                    for keyword in &args.keywords {
                        self.visit_expr(&keyword.value);
                    }
                }

                // Scan decorators
                for decorator in &class_def.decorator_list {
                    self.visit_expr(&decorator.expression);
                }
            }

            // We skip import statements as they're handled separately
            ast::Stmt::Import(_) | ast::Stmt::ImportFrom(_) => {}

            // Other statement types aren't relevant for API usage
            _ => {}
        }
    }

    fn visit_expr(&mut self, expr: &ast::Expr) {
        match expr {
            // Attribute access (e.g., module.symbol)
            ast::Expr::Attribute(attr) => {
                // Check for module.symbol pattern
                self.check_attribute_access(attr);

                // Also scan the value part (for chained attributes)
                self.visit_expr(&attr.value);
            }

            // Name references (e.g., symbol)
            ast::Expr::Name(name) => {
                if name.ctx == ExprContext::Load {
                    self.record_symbol_usage(name.id.as_str());
                }
            }

            // Function calls
            ast::Expr::Call(call) => {
                // Scan the function being called
                self.visit_expr(&call.func);

                // Scan positional arguments
                for arg in &call.arguments.args {
                    self.visit_expr(arg);
                }

                // Scan keyword arguments
                for keyword in &call.arguments.keywords {
                    self.visit_expr(&keyword.value);
                }
            }

            // Binary operations (e.g., a + b)
            ast::Expr::BinOp(binop) => {
                self.visit_expr(&binop.left);
                self.visit_expr(&binop.right);
            }

            // Unary operations (e.g., -x, not y)
            ast::Expr::UnaryOp(unaryop) => {
                self.visit_expr(&unaryop.operand);
            }

            // Comparisons (e.g., a < b)
            ast::Expr::Compare(compare) => {
                self.visit_expr(&compare.left);
                for comp in &compare.comparators {
                    self.visit_expr(comp);
                }
            }

            // Subscript expressions (e.g., list[0], dict['key'])
            ast::Expr::Subscript(subscript) => {
                self.visit_expr(&subscript.value);
                self.visit_expr(&subscript.slice);
            }

            // Containers (list, tuple, dict, set)
            ast::Expr::List(list) => {
                for elt in &list.elts {
                    self.visit_expr(elt);
                }
            }

            ast::Expr::Tuple(tuple) => {
                for elt in &tuple.elts {
                    self.visit_expr(elt);
                }
            }

            ast::Expr::Dict(dict) => {
                for item in &dict.items {
                    if let Some(key) = &item.key {
                        self.visit_expr(key);
                    }
                    self.visit_expr(&item.value);
                }
            }

            ast::Expr::Set(set) => {
                for elt in &set.elts {
                    self.visit_expr(elt);
                }
            }

            // Lambda expressions
            ast::Expr::Lambda(lambda) => {
                self.visit_expr(&lambda.body);
            }

            // Conditional expressions (e.g., x if condition else y)
            ast::Expr::If(ifexp) => {
                self.visit_expr(&ifexp.test);
                self.visit_expr(&ifexp.body);
                self.visit_expr(&ifexp.orelse);
            }

            // Comprehensions
            ast::Expr::ListComp(listcomp) => {
                self.visit_expr(&listcomp.elt);
                for comp in &listcomp.generators {
                    self.visit_expr(&comp.target);
                    self.visit_expr(&comp.iter);
                    for if_expr in &comp.ifs {
                        self.visit_expr(if_expr);
                    }
                }
            }

            // Other expression types aren't relevant for API usage
            _ => {}
        }
    }
}

//------------------------------------------------------------------------------
// Main entry point for the analyze_api command
//------------------------------------------------------------------------------

/// Analyze a Python module or package to determine its effective public API.
///
/// This command examines a Python module or package to identify its "public API" -
/// which means symbols defined in the target and used by external files.
/// It scans for imports and usages to determine which symbols are part of the
/// effective public interface.
///
/// # Arguments
///
/// * `args` - The command line arguments specific to the analysis
/// * `config_arguments` - General Ruff configuration arguments
///
/// # Returns
///
/// * `Result<ExitStatus>` - Success if analysis completes without errors
pub fn analyze_api(
    args: &AnalyzeApiArgs,
    config_arguments: &ConfigArguments,
) -> Result<ExitStatus> {
    // Resolve project configuration
    let pyproject_config = resolve::resolve(config_arguments, None)?;
    let _settings = &pyproject_config.settings;

    info!("Analyzing API for: {}", args.target_path.display());
    if args.no_parallel {
        info!("Parallel processing disabled, using sequential implementation");
    } else {
        info!("Using parallel processing for file analysis");
    }

    // Check if the target path exists and is accessible
    if !args.target_path.exists() {
        anyhow::bail!("Target path does not exist: {}", args.target_path.display());
    }

    // Determine target boundary
    let target_boundary = determine_target_boundary(&args.target_path)?;

    // Find all Python files in the project (including the target)
    let project_root = if let Some(root) = &args.project_root {
        info!("Using explicit project root: {}", root.display());
        root.clone()
    } else {
        let detected_root = detect_project_root(&args.target_path)?;
        info!("Auto-detected project root: {}", detected_root.display());
        detected_root
    };

    // Check if target is within the project root
    let target_canonical = fs::canonicalize(&args.target_path)?;
    let project_canonical = fs::canonicalize(&project_root)?;
    let is_target_within_project = target_canonical.starts_with(&project_canonical);

    if !is_target_within_project {
        info!(
            "Target {} is outside project root {}",
            target_canonical.display(),
            project_canonical.display()
        );
    }

    let files = resolve_default_files(vec![project_root.to_path_buf()], false);
    let (paths, resolver) = python_files_in_path(&files, &pyproject_config, config_arguments)?;

    if paths.is_empty() {
        info!("No Python files found in the project");
        return Ok(ExitStatus::Success);
    }

    debug!("Found {} Python files in the project", paths.len());

    // Collect all project Python files and divide them into target and external files
    let mut target_files = Vec::new();
    let mut external_files = Vec::new();

    // First, ensure the target file(s) are included for analysis, even if outside project root
    if !is_target_within_project {
        // If target is a file, add it directly
        if args.target_path.is_file() && is_python_file(&args.target_path) {
            let canonical_path = fs::canonicalize(&args.target_path)?;
            debug!("Adding external target file: {}", canonical_path.display());

            // Create a ResolvedFile for the target
            let resolved_file = ResolvedFile::Root(canonical_path.clone());
            target_files.push((canonical_path, resolved_file));
        }
        // If target is a directory, scan it for Python files
        else if args.target_path.is_dir() {
            for entry in WalkDir::new(&args.target_path)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if is_python_file(path) {
                    let canonical_path = fs::canonicalize(path)?;
                    debug!(
                        "Adding external target directory file: {}",
                        canonical_path.display()
                    );

                    // Create a ResolvedFile for each Python file
                    let resolved_file = ResolvedFile::Nested(canonical_path.clone());
                    target_files.push((canonical_path, resolved_file));
                }
            }
        }
    }

    // Now process all files from the project
    for resolved_result in paths {
        if let Ok(resolved_file) = resolved_result {
            let path = resolved_file.path().to_path_buf();
            trace!("Considering path: {}", path.display());

            // Determine if this file is within the target boundary
            if is_file_within_target(&target_boundary, &path) {
                trace!("Added to target files: {}", path.display());
                target_files.push((path.clone(), resolved_file));
            } else {
                trace!("Added to external files: {}", path.display());
                external_files.push((path.clone(), resolved_file));
            }
        }
    }

    debug!(
        "Found {} target files and {} external files",
        target_files.len(),
        external_files.len()
    );

    if target_files.is_empty() {
        info!("No Python files found in the target path");
        return Ok(ExitStatus::Success);
    }

    // Extract candidate symbols from target files
    let candidate_symbols = extract_candidate_symbols(&target_files, &resolver)?;

    debug!(
        "Found {} candidate symbols in target",
        candidate_symbols.len()
    );

    // Determine the target module name for more accurate attribute accesses tracking
    let target_module_name = determine_target_module_name(&candidate_symbols);

    // Use the semantic model approach for more accurate attribute access detection
    let public_api = analyze_external_with_semantic_model(
        &candidate_symbols,
        &external_files,
        &resolver,
        &target_module_name,
        args.no_parallel,
    )?;

    // Output the results
    output_results(&public_api, args)?;

    Ok(ExitStatus::Success)
}

//------------------------------------------------------------------------------
// Project and file analysis functions
//------------------------------------------------------------------------------

/// Analyze external files using semantic model approach with parallelism
fn analyze_external_with_semantic_model(
    candidates: &HashMap<String, DefinedSymbol>,
    external_files: &ResolvedFileCollection,
    _resolver: &Resolver,
    target_module_name: &str,
    no_parallel: bool,
) -> Result<Vec<ApiSymbol>> {
    // Create an ApiAnalyzer instance to manage shared state
    let analyzer = ApiAnalyzer::new(candidates.clone(), target_module_name.to_string());

    // This check uses the no_parallel parameter directly instead of analyzer.no_parallel
    if no_parallel {
        // Process external files sequentially
        debug!(
            "Processing {} external files sequentially",
            external_files.len()
        );
        external_files.iter().for_each(|(path, resolved_file)| {
            debug!("Analyzing external file: {}", path.display());

            // Read and parse the file content
            match std::fs::read_to_string(resolved_file.path()) {
                Ok(file_content) => {
                    if let Ok(parsed) = ruff_python_parser::parse_module(&file_content) {
                        // Create per-file analysis state
                        let mut file_state = FileAnalysisState::new();

                        // Create a visitor for this file
                        let mut visitor = ApiAnalyzerVisitor::new(path, &analyzer, &mut file_state);

                        // First pass: identify module imports and aliases
                        visitor.process_imports(&parsed.syntax().body);

                        // Second pass: use the visitor to scan the entire module for API usage
                        for stmt in &parsed.syntax().body {
                            visitor.visit_stmt(stmt);
                        }
                    }
                }
                Err(e) => {
                    debug!("Error reading file {}: {}", path.display(), e);
                }
            }
        });
    } else {
        // Process external files in parallel
        debug!(
            "Processing {} external files in parallel",
            external_files.len()
        );
        external_files.par_iter().for_each(|(path, resolved_file)| {
            debug!("Analyzing external file: {}", path.display());

            // Read and parse the file content
            match std::fs::read_to_string(resolved_file.path()) {
                Ok(file_content) => {
                    if let Ok(parsed) = ruff_python_parser::parse_module(&file_content) {
                        // Create per-file analysis state
                        let mut file_state = FileAnalysisState::new();

                        // Create a visitor for this file
                        let mut visitor = ApiAnalyzerVisitor::new(path, &analyzer, &mut file_state);

                        // First pass: identify module imports and aliases
                        visitor.process_imports(&parsed.syntax().body);

                        // Second pass: use the visitor to scan the entire module for API usage
                        for stmt in &parsed.syntax().body {
                            visitor.visit_stmt(stmt);
                        }
                    }
                }
                Err(e) => {
                    debug!("Error reading file {}: {}", path.display(), e);
                }
            }
        });
    }

    // Return the API symbols
    analyzer.build_api_symbols()
}

/// Output the results of the API analysis
fn output_results(public_api: &[ApiSymbol], args: &AnalyzeApiArgs) -> Result<()> {
    use colored::Colorize;

    // Handle the short output format first
    if args.short {
        if public_api.is_empty() {
            println!("No public API symbols found with external usage.");
            return Ok(());
        }

        // Clone and sort by usage count (descending)
        let mut sorted_api = public_api.to_vec();
        sorted_api.sort_by(|a, b| b.usage_count.cmp(&a.usage_count));

        println!(
            "Public API Summary for {}:",
            args.target_path.display().to_string().bold()
        );
        for symbol in sorted_api {
            // Create the usage string (String for multiple usages)
            let usage_output = if symbol.usage_count == 1 {
                "1 external usage".to_string()
            } else {
                format!("{} external usages", symbol.usage_count)
            };
            // Print using the owned String or a string literal
            println!("  {} ({})", symbol.name.cyan(), usage_output);
        }
        return Ok(());
    }

    // Determine the output format for non-short output
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

            for kind in &kinds {
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

/// Determine the project root by looking for pyproject.toml or package markers
fn detect_project_root(target_path: &Path) -> Result<PathBuf> {
    let mut current_dir = if target_path.is_file() {
        target_path.parent().map(Path::to_path_buf)
    } else {
        Some(target_path.to_path_buf())
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
        current_dir = dir.parent().map(Path::to_path_buf);
    }

    // If no project markers found, default to the parent directory of the target
    Ok(if target_path.is_file() {
        target_path
            .parent()
            .map_or_else(|| Path::new(".").to_path_buf(), Path::to_path_buf)
    } else {
        target_path.to_path_buf()
    })
}

/// Check if a path is a Python file
fn is_python_file(path: &Path) -> bool {
    path.is_file()
        && path.extension().map_or(false, |ext| {
            ext.eq_ignore_ascii_case("py") || ext.eq_ignore_ascii_case("pyi")
        })
}

/// Determine the target boundary of the target module/package
fn determine_target_boundary(target_path: &Path) -> Result<Vec<PathBuf>> {
    debug!("Target path: {}", target_path.display());

    let mut boundary = Vec::new();

    if target_path.is_file() {
        debug!("Target is a file, adding to boundary");
        let normalized_path = match fs::canonicalize(target_path) {
            Ok(path) => path,
            Err(e) => {
                debug!("Error normalizing path {}: {}", target_path.display(), e);
                target_path.to_path_buf()
            }
        };
        trace!("Normalized path: {}", normalized_path.display());

        // Check if it's a Python file
        if is_python_file(target_path) {
            boundary.push(normalized_path);
        } else {
            debug!("Not a Python file, skipping");
        }
    } else if target_path.is_dir() {
        debug!("Target is a directory, scanning for Python files");
        // Recursively find all Python files in the directory
        for entry in WalkDir::new(target_path).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            trace!("Checking path: {}", path.display());
            if is_python_file(path) {
                trace!("Adding Python file to boundary: {}", path.display());
                match fs::canonicalize(path) {
                    Ok(canonical_path) => boundary.push(canonical_path),
                    Err(e) => {
                        debug!("Error normalizing path {}: {}", path.display(), e);
                        boundary.push(path.to_path_buf());
                    }
                }
            }
        }
    }

    debug!("Found {} files in boundary", boundary.len());
    Ok(boundary)
}

/// Check whether a file is within the target boundary, using canonical paths for comparison.
fn is_file_within_target(boundary: &[PathBuf], file_path: &Path) -> bool {
    // Get canonical path for the file being checked
    let canonical_file_path =
        fs::canonicalize(file_path).unwrap_or_else(|_| file_path.to_path_buf());

    trace!(
        "Checking if {} is in boundary",
        canonical_file_path.display()
    );

    if boundary.len() == 1 {
        // Single file case
        let canonical_boundary_path = &boundary[0];
        let result = canonical_file_path == *canonical_boundary_path;
        trace!(
            "Single file comparison: {} == {} ? {}",
            canonical_file_path.display(),
            canonical_boundary_path.display(),
            result
        );
        result
    } else {
        // Multiple files case
        for boundary_path in boundary {
            if &canonical_file_path == boundary_path {
                trace!(
                    "Multi-file match: {} == {}",
                    canonical_file_path.display(),
                    boundary_path.display()
                );
                return true;
            }
        }
        false
    }
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

                        // Construct fully qualified name directly
                        let mut fully_qualified_name = module_name.clone();
                        fully_qualified_name.push('.');
                        fully_qualified_name.push_str(name);

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

                        // Construct fully qualified name directly
                        let mut fully_qualified_name = module_name.clone();
                        fully_qualified_name.push('.');
                        fully_qualified_name.push_str(name);

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
fn get_module_name_from_path(path: &Path) -> String {
    // Get the canonical path if possible to avoid relative path issues
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

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

/// Determine the target module name from candidate symbols
fn determine_target_module_name(candidate_symbols: &HashMap<String, DefinedSymbol>) -> String {
    if let Some((_, def)) = candidate_symbols.iter().next() {
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
    }
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

    #[test]
    fn test_import_with_different_module_path() -> Result<()> {
        // Create a temporary directory structure for testing
        let temp_dir = tempdir()?;
        let project_dir = temp_dir.path().join("test_project");
        fs::create_dir_all(&project_dir)?;

        // Create a package structure
        let package_x_dir = project_dir.join("x");
        let package_y_dir = package_x_dir.join("y");
        fs::create_dir_all(&package_y_dir)?;

        // Create __init__.py files
        File::create(project_dir.join("__init__.py"))?;
        File::create(package_x_dir.join("__init__.py"))?;
        File::create(package_y_dir.join("__init__.py"))?;

        // Create module with a symbol
        let module_path = package_y_dir.join("module.py");
        fs::write(&module_path, "my_symbol = 'test value'")?;

        // Create a file that imports the symbol via a different path
        let importer_path = project_dir.join("importer.py");
        fs::write(&importer_path, "from a.b import my_symbol")?;

        // Create candidate symbol with fully qualified name
        let mut candidates = HashMap::new();
        candidates.insert(
            "my_symbol".to_string(),
            DefinedSymbol {
                kind: SymbolKind::Variable,
                location: module_path.clone(),
                docstring: None,
                is_public: true,
                fully_qualified_name: "x.y.module.my_symbol".to_string(),
            },
        );

        // Create analyzer with the correct number of parameters
        let analyzer = ApiAnalyzer::new(
            candidates,
            "x".to_string(), // Target module name
        );

        // Process the import statement
        let file_content = fs::read_to_string(&importer_path)?;
        let parsed = ruff_python_parser::parse_module(&file_content);

        if let Ok(parsed) = parsed {
            // Create file state
            let mut file_state = FileAnalysisState::new();

            // Create a visitor for this file
            let mut visitor = ApiAnalyzerVisitor::new(&importer_path, &analyzer, &mut file_state);

            // Process imports in the file
            visitor.process_imports(&parsed.syntax().body);

            // The symbol should NOT be counted since the import path doesn't match
            let final_usage = match Arc::try_unwrap(analyzer.usage_counts) {
                Ok(mutex) => mutex.into_inner()?,
                Err(arc) => arc
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Failed to acquire lock"))?
                    .clone(),
            };

            let usage = final_usage.get("my_symbol").unwrap();

            // Verify that usage count is still 0 since "a.b.my_symbol" != "x.y.module.my_symbol"
            assert_eq!(usage.0, 0);
            assert!(usage.1.is_empty());
        }

        Ok(())
    }
}
