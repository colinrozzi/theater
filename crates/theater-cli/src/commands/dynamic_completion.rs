use clap::Parser;
use tracing::debug;

use crate::error::CliResult;
use crate::CommandContext;

#[derive(Debug, Parser)]
pub struct DynamicCompletionArgs {
    /// The command line being completed
    #[arg(required = true)]
    pub line: String,

    /// The current word being completed
    #[arg(required = true)]
    pub current: String,
}

/// Generate dynamic completions based on current theater state
pub async fn execute_async(args: &DynamicCompletionArgs, _ctx: &CommandContext) -> CliResult<()> {
    debug!("Generating dynamic completion for: '{}'", args.line);
    debug!("Current word: '{}'", args.current);

    let completions = generate_dynamic_completions(args).await?;

    for completion in completions {
        println!("{}", completion);
    }

    Ok(())
}

/// Generate completions based on context
async fn generate_dynamic_completions(
    args: &DynamicCompletionArgs,
) -> CliResult<Vec<String>> {
    let words: Vec<&str> = args.line.split_whitespace().collect();

    match words.as_slice() {
        // theater <command>
        ["theater"] => Ok(get_command_completions(&args.current)),

        // theater start <manifest>
        ["theater", "start"] => get_manifest_completions(&args.current).await,

        // theater create <template>
        ["theater", "create"] => Ok(get_template_completions(&args.current)),

        // theater completion <shell>
        ["theater", "completion"] => Ok(get_shell_completions(&args.current)),

        _ => Ok(vec![]),
    }
}

/// Get available command completions
fn get_command_completions(current: &str) -> Vec<String> {
    let commands = vec!["build", "completion", "create", "start"];

    commands
        .into_iter()
        .filter(|cmd| cmd.starts_with(current))
        .map(|s| s.to_string())
        .collect()
}

/// Get template completions
fn get_template_completions(current: &str) -> Vec<String> {
    let templates = vec!["basic", "message-server", "supervisor"];

    templates
        .into_iter()
        .filter(|tmpl| tmpl.starts_with(current))
        .map(|s| s.to_string())
        .collect()
}

/// Get shell completions
fn get_shell_completions(current: &str) -> Vec<String> {
    let shells = vec!["bash", "zsh", "fish", "powershell", "elvish"];

    shells
        .into_iter()
        .filter(|shell| shell.starts_with(current))
        .map(|s| s.to_string())
        .collect()
}

/// Get manifest file completions
async fn get_manifest_completions(current: &str) -> CliResult<Vec<String>> {
    let mut completions = Vec::new();

    if let Ok(entries) = std::fs::read_dir(".") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name == "manifest.toml" || name.ends_with(".toml") {
                    if name.starts_with(current) {
                        completions.push(name.to_string());
                    }
                }
            }
        }
    }

    Ok(completions)
}
