use clap::{CommandFactory, Parser};
use clap_complete::{generate, Shell};
use std::io;
use tracing::debug;

use crate::error::{CliError, CliResult};
use crate::CommandContext;

#[derive(Debug, Parser)]
#[command(
    long_about = "Generate shell completion scripts.\n\nYou typically only need to re-run this after updating the theater CLI or changing the completion file location."
)]
pub struct CompletionArgs {
    /// Shell to generate completions for
    #[arg(value_enum)]
    pub shell: Shell,

    /// Output file (defaults to stdout)
    #[arg(short, long)]
    pub output: Option<std::path::PathBuf>,
}

/// Generate shell completion scripts
pub async fn execute_async(args: &CompletionArgs, ctx: &CommandContext) -> CliResult<()> {
    debug!("Generating shell completion for: {:?}", args.shell);

    let mut app = crate::Cli::command();
    let app_name = app.get_name().to_string();

    match &args.output {
        Some(output_path) => {
            debug!("Writing completion to file: {:?}", output_path);

            let mut file = std::fs::File::create(output_path).map_err(|e| CliError::IoError {
                operation: format!("create completion file: {}", output_path.display()),
                source: e,
            })?;

            generate(args.shell, &mut app, &app_name, &mut file);

            ctx.output.success(&format!(
                "Shell completion for {} written to: {}",
                args.shell,
                output_path.display()
            ))?;
        }
        None => {
            debug!("Writing completion to stdout");
            generate(args.shell, &mut app, &app_name, &mut io::stdout());
        }
    }

    // Only show installation instructions when writing to a file
    // Don't show them when output goes to stdout (for eval)
    if args.output.is_some() && !ctx.json {
        show_installation_instructions(args.shell, ctx)?;
    }

    Ok(())
}

/// Show installation instructions for the generated completion script
fn show_installation_instructions(shell: Shell, ctx: &CommandContext) -> CliResult<()> {
    let instructions = match shell {
        Shell::Bash => {
            r#"
To install bash completions:

1. Save the completion script:
   theater completion bash > ~/.local/share/bash-completion/completions/theater

2. Or add to your ~/.bashrc:
   eval "$(theater completion bash)"

3. Restart your shell or run:
   source ~/.bashrc
"#
        }
        Shell::Zsh => {
            r#"
To install zsh completions:

1. Save the completion script to a directory in your $fpath:
   theater completion zsh > ~/.local/share/zsh/site-functions/_theater

2. Or add to your ~/.zshrc:
   eval "$(theater completion zsh)"

3. Restart your shell or run:
   source ~/.zshrc
"#
        }
        Shell::Fish => {
            r#"
To install fish completions:

1. Save the completion script:
   theater completion fish > ~/.config/fish/completions/theater.fish

2. Or add to your fish config:
   theater completion fish | source

3. Restart your shell
"#
        }
        Shell::PowerShell => {
            r#"
To install PowerShell completions:

1. Add to your PowerShell profile:
   theater completion powershell | Out-String | Invoke-Expression

2. Or save to a file and dot-source it in your profile:
   theater completion powershell > theater_completion.ps1
   . .\theater_completion.ps1
"#
        }
        Shell::Elvish => {
            r#"
To install Elvish completions:

1. Add to your ~/.config/elvish/rc.elv:
   eval (theater completion elvish | slurp)
"#
        }
        _ => "Please refer to your shell's documentation for completion installation.",
    };

    ctx.output.info(instructions)?;
    Ok(())
}
