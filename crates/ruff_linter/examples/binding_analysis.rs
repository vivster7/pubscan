use std::path::Path;

use ruff_linter::{
    analyze_source_code, package::PackageRoot, TopLevelBindingType,
};

fn main() {
    // Example Python code to analyze
    let python_code = r#"
import os
from collections import defaultdict
import numpy as np
from pathlib import Path as PathLib  # Aliased import

# Local variables and functions
x = 10
y = 20  # This variable is not used

def hello_world():
    print("Hello, World!")
    return x  # Uses x from global scope

class MyClass:
    def __init__(self):
        self.value = x  # Uses x from global scope
        
# Using the imports
def process_files(directory):
    for file in os.listdir(directory):
        if PathLib(file).suffix == '.py':
            print(f"Found Python file: {file}")
    
    # Using numpy
    arr = np.array([1, 2, 3])
    print(arr.mean())
    
    # Using defaultdict
    counts = defaultdict(int)
    counts['python'] += 1
    return counts
"#;

    // Create a dummy path for analysis
    let path = Path::new("/tmp/example.py");
    let package = PackageRoot::root(Path::new("/tmp"));
    
    // Analyze the code
    let bindings = analyze_source_code(path, package, python_code);
    
    // Print the results
    println!("Top-level bindings with usage information:");
    println!("{:<15} {:<30} {:<10} {:<15}", "Name", "Qualified Name", "Usage", "Type");
    println!("{:<15} {:<30} {:<10} {:<15}", "----", "-------------", "-----", "----");
    
    for binding in bindings {
        let binding_type = match binding.binding_type {
            TopLevelBindingType::Imported => "Imported",
            TopLevelBindingType::LocallyDefined => "Local",
        };
        
        println!(
            "{:<15} {:<30} {:<10} {:<15}",
            binding.name,
            binding.qualified_name.unwrap_or_default(),
            binding.usage_count,
            binding_type
        );
    }
}