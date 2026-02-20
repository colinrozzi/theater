//! Test program for Pact parsing and code generation
//!
//! Run with: cargo run -p pact-test

use pack::{generate_rust, parse_pact_dir, PactExport};

fn main() {
    // Parse the pact directory relative to this package
    let pact_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("pact");

    println!("Parsing pact directory: {}", pact_dir.display());
    println!();

    match parse_pact_dir(&pact_dir) {
        Ok(root) => {
            println!("Parsed {} interfaces:", root.children.len());

            for child in &root.children {
                println!();
                println!("=== {} ===", child.name);

                // Print version if available
                if let Some(version) = child.version() {
                    println!("  version: {}", version);
                }

                // Print other metadata
                for meta in &child.metadata {
                    if meta.name != "version" {
                        println!("  @{}: {:?}", meta.name, meta.value);
                    }
                }

                // Print type params (generics)
                if !child.type_params.is_empty() {
                    println!("  type params:");
                    for tp in &child.type_params {
                        if let Some(constraint) = &tp.constraint {
                            println!("    {}: {}", tp.name, constraint);
                        } else {
                            println!("    {}", tp.name);
                        }
                    }
                }

                // Print types
                if !child.types.is_empty() {
                    println!("  types:");
                    for typedef in &child.types {
                        println!("    {}", typedef.name());
                    }
                }

                // Print imports
                if !child.imports.is_empty() {
                    println!("  imports:");
                    for import in &child.imports {
                        println!("    {:?}", import);
                    }
                }

                // Print exports
                if !child.exports.is_empty() {
                    println!("  exports:");
                    for export in &child.exports {
                        match export {
                            PactExport::Function(f) => {
                                let params: Vec<_> = f.params.iter()
                                    .map(|p| format!("{}: {:?}", p.name, p.ty))
                                    .collect();
                                let results: Vec<_> = f.results.iter()
                                    .map(|r| format!("{:?}", r))
                                    .collect();
                                if results.is_empty() {
                                    println!("    func {}({})", f.name, params.join(", "));
                                } else {
                                    println!("    func {}({}) -> {}", f.name, params.join(", "), results.join(", "));
                                }
                            }
                            PactExport::Type(t) => {
                                println!("    type {}", t.name());
                            }
                        }
                    }
                }

                // Print nested interfaces
                if !child.children.is_empty() {
                    println!("  nested interfaces:");
                    for nested in &child.children {
                        println!("    {}", nested.name);
                    }
                }
            }

            // Convert to Arena and show structure
            println!();
            println!("=== Arena conversion ===");
            for child in &root.children {
                let arena = child.to_arena();
                println!("{}: {} types, {} functions, {} children",
                    arena.name,
                    arena.types.len(),
                    arena.functions.len(),
                    arena.children.len()
                );
            }

            // Generate Rust code
            println!();
            println!("=== Code Generation ===");
            for child in &root.children {
                println!();
                println!("--- {}.rs ---", child.name);
                let code = generate_rust(child);
                println!("{}", code);
            }
        }
        Err(e) => {
            eprintln!("Error parsing pact directory: {}", e);
            std::process::exit(1);
        }
    }
}
