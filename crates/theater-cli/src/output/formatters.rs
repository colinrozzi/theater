use crate::error::CliResult;
use crate::output::{OutputFormat, OutputManager};

/// Build result formatter
#[derive(Debug, serde::Serialize)]
pub struct BuildResult {
    pub success: bool,
    pub project_dir: std::path::PathBuf,
    pub wasm_path: Option<std::path::PathBuf>,
    pub manifest_exists: bool,
    pub manifest_path: Option<std::path::PathBuf>,
    pub build_type: String,
    pub package_name: String,
    pub stdout: String,
    pub stderr: String,
}

impl OutputFormat for BuildResult {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        if self.success {
            output.success("Build completed successfully")?;
            if let Some(wasm_path) = &self.wasm_path {
                println!(
                    "  Package: {}",
                    output.theme().accent().apply_to(wasm_path.display())
                );
            }
        } else {
            output.error("Build failed")?;
            if !self.stderr.is_empty() {
                println!("{}", output.theme().muted().apply_to(&self.stderr));
            }
        }
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        if self.success {
            println!(
                "{} {}",
                output.theme().success_icon(),
                output.theme().highlight().apply_to("Build Successful")
            );
            println!();
            println!(
                "Package: {}",
                output.theme().accent().apply_to(&self.package_name)
            );
            println!(
                "Build Type: {}",
                output.theme().muted().apply_to(&self.build_type)
            );

            if let Some(wasm_path) = &self.wasm_path {
                println!(
                    "WASM: {}",
                    output.theme().accent().apply_to(wasm_path.display())
                );
            }

            if self.manifest_exists {
                if let Some(manifest_path) = &self.manifest_path {
                    println!("\nTo run your actor:");
                    println!(
                        "  theater start {}",
                        output.theme().muted().apply_to(manifest_path.display())
                    );
                }
            } else {
                println!(
                    "\n{} No manifest.toml found.",
                    output.theme().warning_icon()
                );
            }

            if !self.stdout.is_empty() {
                println!("\nBuild Output:");
                println!("{}", output.theme().muted().apply_to(&self.stdout));
            }
        } else {
            println!(
                "{} {}",
                output.theme().error_icon(),
                output.theme().error().apply_to("Build Failed")
            );

            if !self.stderr.is_empty() {
                println!("\nError Output:");
                println!("{}", self.stderr);
            }
            if !self.stdout.is_empty() {
                println!("\nBuild Output:");
                println!("{}", self.stdout);
            }
        }
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Property", "Value"];
        let mut rows = vec![vec![
            "Status".to_string(),
            if self.success {
                "Success".to_string()
            } else {
                "Failed".to_string()
            },
        ]];

        rows.push(vec!["Package".to_string(), self.package_name.clone()]);
        rows.push(vec!["Build Type".to_string(), self.build_type.clone()]);
        rows.push(vec![
            "Project Dir".to_string(),
            self.project_dir.display().to_string(),
        ]);

        if let Some(wasm_path) = &self.wasm_path {
            rows.push(vec![
                "WASM".to_string(),
                wasm_path.display().to_string(),
            ]);
        }

        rows.push(vec![
            "Manifest Exists".to_string(),
            self.manifest_exists.to_string(),
        ]);

        if let Some(manifest_path) = &self.manifest_path {
            rows.push(vec![
                "Manifest Path".to_string(),
                manifest_path.display().to_string(),
            ]);
        }

        output.table(&headers, &rows)?;
        Ok(())
    }

    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}

/// Project creation formatter
#[derive(Debug, serde::Serialize)]
pub struct ProjectCreated {
    pub name: String,
    pub template: String,
    pub path: std::path::PathBuf,
    pub build_instructions: Vec<String>,
}

impl OutputFormat for ProjectCreated {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{} Created project: {}",
            output.theme().success().apply_to("✓"),
            output.theme().accent().apply_to(&self.name)
        );
        println!(
            "Path: {}",
            output.theme().muted().apply_to(self.path.display())
        );
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{} {}",
            output.theme().success().apply_to("✓"),
            output
                .theme()
                .highlight()
                .apply_to(&format!("Created new actor project: {}", self.name))
        );
        println!();
        println!(
            "Template: {}",
            output.theme().accent().apply_to(&self.template)
        );
        println!(
            "Location: {}",
            output.theme().muted().apply_to(self.path.display())
        );
        println!();
        println!("{}", output.theme().highlight().apply_to("Next steps:"));
        for (i, instruction) in self.build_instructions.iter().enumerate() {
            println!(
                "  {}. {}",
                i + 1,
                output.theme().muted().apply_to(instruction)
            );
        }
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Property", "Value"];
        let rows = vec![
            vec!["Name".to_string(), self.name.clone()],
            vec!["Template".to_string(), self.template.clone()],
            vec!["Path".to_string(), self.path.display().to_string()],
        ];
        output.table(&headers, &rows)?;

        println!();
        println!(
            "{}",
            output.theme().highlight().apply_to("Build Instructions:")
        );
        for (i, instruction) in self.build_instructions.iter().enumerate() {
            println!("  {}. {}", i + 1, instruction);
        }
        Ok(())
    }

    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}
