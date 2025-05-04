use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use log::debug;

/// CLI arguments for the standalone API analyzer
#[derive(Debug, Parser)]
#[command(
    name = "api-analyzer",
    about = "Analyze a Python module or package to determine its effective public API based on actual usage.",
    version
)]
struct Args {
    /// The path to the Python module (.py file) or package (directory) to analyze.
    #[clap()]
    target: PathBuf,

    /// The output format to use (text/json).
    #[clap(long = "output-format", short = 'o', default_value = "text")]
    output_format: String,

    /// The path to the Python executable to use for venv parsing.
    #[clap(long = "python")]
    python: Option<PathBuf>,

    /// Explicitly specify the project root directory (default: auto-detected from target).
    #[clap(long = "project-root")]
    project_root: Option<PathBuf>,

    /// Disable parallel processing for file analysis.
    #[clap(long)]
    no_parallel: bool,

    /// Increase verbosity (can be used multiple times)
    #[clap(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Output only a sorted summary line for each symbol.
    #[clap(long)]
    short: bool,

    /// Don't exclude test files from analysis. By default, files in 'test' or 'tests' directories
    /// and files with names starting with 'test_' or ending with '_test.py' are excluded.
    #[clap(long = "no-ignore-test-files")]
    no_ignore_test_files: bool,
}

fn main() -> ExitCode {
    // Set up colored output
    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).unwrap_or(());

    // Parse command line arguments
    let args = Args::parse();

    // Set up logging based on verbosity
    let log_level = match args.verbose {
        0 => log::LevelFilter::Info,
        1 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    };

    env_logger::Builder::new()
        .filter_level(log_level)
        .format_timestamp(None)
        .init();

    // Convert relative paths to absolute paths
    let target_abs = if args.target.is_relative() {
        match env::current_dir() {
            Ok(current_dir) => current_dir.join(&args.target),
            Err(e) => {
                eprintln!("Error getting current directory: {}", e);
                return ExitCode::from(1);
            }
        }
    } else {
        args.target.clone()
    };

    // Convert project_root to absolute path if it's provided and relative
    let project_root_abs = args.project_root.map(|path| {
        if path.is_relative() {
            match env::current_dir() {
                Ok(current_dir) => current_dir.join(path),
                Err(_) => path, // Fall back to the relative path
            }
        } else {
            path
        }
    });

    // Convert python executable path to absolute if it's provided and relative
    let python_abs = args.python.map(|path| {
        if path.is_relative() {
            match env::current_dir() {
                Ok(current_dir) => current_dir.join(path),
                Err(_) => path, // Fall back to the relative path
            }
        } else {
            path
        }
    });

    debug!("Using target path: {}", target_abs.display());
    if let Some(ref root) = project_root_abs {
        debug!("Using project root: {}", root.display());
    }

    // Parse analyzer command using partition
    let analyze_cmd = ruff::args::AnalyzeApiCommand {
        target: target_abs,
        output_format: Some(args.output_format),
        python: python_abs,
        project_root: project_root_abs,
        preview: false,
        no_preview: false,
        detect_string_imports: false,
        target_version: None,
        no_parallel: args.no_parallel,
        short: args.short,
        no_ignore_test_files: args.no_ignore_test_files,
    };

    // Use Default implementation and rely on ExplicitConfigOverrides for more settings
    let mut global_config = ruff::args::GlobalConfigArgs::default();
    global_config.isolated = true; // Don't try to use .ruff.toml or pyproject.toml
    global_config.config = Vec::new(); // No config options provided

    match analyze_cmd.partition(global_config) {
        Ok((analyze_args, config_args)) => {
            // Call into ruff's analyze_api function
            match run_analyze_api(analyze_args, config_args) {
                Ok(exit_status) => exit_status.into(),
                Err(err) => {
                    eprintln!("Error: {}", err);
                    ExitCode::from(1)
                }
            }
        }
        Err(err) => {
            eprintln!("Error: {}", err);
            ExitCode::from(1)
        }
    }
}

/// Wrapper function to call ruff's analyze_api functionality
fn run_analyze_api(
    args: ruff::args::AnalyzeApiArgs,
    config_args: ruff::args::ConfigArguments,
) -> Result<ruff::ExitStatus> {
    debug!(
        "Running API analysis on target: {}",
        args.target_path.display()
    );
    ruff::commands::analyze_api::analyze_api(&args, &config_args)
}
