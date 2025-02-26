#[cfg(test)]
mod tests {
    use crate::cli::{Args, Commands};
    use clap::Parser;

    #[test]
    fn test_cli_parsing() {
        // Test basic command
        let args = Args::parse_from(&["theater", "list"]);
        match args.command {
            Commands::List { detailed, address } => {
                assert_eq!(detailed, false);
                assert_eq!(address, "127.0.0.1:9000");
            }
            _ => panic!("Expected List command"),
        }

        // Test manifest command
        let args = Args::parse_from(&["theater", "manifest", "list"]);
        match args.command {
            Commands::Manifest(cmd) => {
                match cmd {
                    crate::cli::manifest::ManifestCommands::List { detailed } => {
                        assert_eq!(detailed, false);
                    }
                    _ => panic!("Expected Manifest List command"),
                }
            }
            _ => panic!("Expected Manifest command"),
        }

        // Test actor command
        let args = Args::parse_from(&["theater", "actor", "list", "--detailed"]);
        match args.command {
            Commands::Actor(cmd) => {
                match cmd {
                    crate::cli::actor::ActorCommands::List { detailed } => {
                        assert_eq!(detailed, true);
                    }
                    _ => panic!("Expected Actor List command"),
                }
            }
            _ => panic!("Expected Actor command"),
        }

        // Test system command
        let args = Args::parse_from(&["theater", "system", "status", "--detailed"]);
        match args.command {
            Commands::System(cmd) => {
                match cmd {
                    crate::cli::system::SystemCommands::Status { detailed, watch, interval } => {
                        assert_eq!(detailed, true);
                        assert_eq!(watch, false);
                        assert_eq!(interval, 2);
                    }
                    _ => panic!("Expected System Status command"),
                }
            }
            _ => panic!("Expected System command"),
        }

        // Test dev command
        let args = Args::parse_from(&["theater", "dev", "build", "--release"]);
        match args.command {
            Commands::Dev(cmd) => {
                match cmd {
                    crate::cli::dev::DevCommands::Build { path, output, release } => {
                        assert_eq!(release, true);
                        assert_eq!(path, None);
                        assert_eq!(output, None);
                    }
                    _ => panic!("Expected Dev Build command"),
                }
            }
            _ => panic!("Expected Dev command"),
        }
    }
}
