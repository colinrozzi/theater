# Enhanced fish completion for theater CLI with dynamic completions

# Main command completions
complete -c theater -f

# Global options
complete -c theater -s v -l verbose -d "Turn on verbose output"
complete -c theater -l json -d "Display output in JSON format"

# Subcommands
complete -c theater -n "__fish_use_subcommand" -a "build" -d "Build a Theater actor to WebAssembly"
complete -c theater -n "__fish_use_subcommand" -a "channel" -d "Channel operations"
complete -c theater -n "__fish_use_subcommand" -a "completion" -d "Generate shell completion scripts"
complete -c theater -n "__fish_use_subcommand" -a "create" -d "Create a new Theater actor project"
complete -c theater -n "__fish_use_subcommand" -a "events" -d "Get actor events"
complete -c theater -n "__fish_use_subcommand" -a "inspect" -d "Inspect a running actor"
complete -c theater -n "__fish_use_subcommand" -a "list" -d "List all running actors"
complete -c theater -n "__fish_use_subcommand" -a "list-stored" -d "List stored actor IDs"
complete -c theater -n "__fish_use_subcommand" -a "message" -d "Send a message to an actor"
complete -c theater -n "__fish_use_subcommand" -a "start" -d "Start or deploy an actor from a manifest"
complete -c theater -n "__fish_use_subcommand" -a "state" -d "Get actor state"
complete -c theater -n "__fish_use_subcommand" -a "stop" -d "Stop a running actor"
complete -c theater -n "__fish_use_subcommand" -a "subscribe" -d "Subscribe to real-time events from an actor"

# Create command completions
complete -c theater -n "__fish_seen_subcommand_from create" -a "basic http" -d "Project template"

# Completion command completions
complete -c theater -n "__fish_seen_subcommand_from completion" -a "bash zsh fish powershell elvish" -d "Shell type"

# Channel subcommands
complete -c theater -n "__fish_seen_subcommand_from channel" -a "open" -d "Open a communication channel"

# Dynamic completions for actor IDs
function __theater_dynamic_completion
    set -l cmdline (commandline -cp)
    set -l current (commandline -ct)
    theater dynamic-completion "$cmdline" "$current" 2>/dev/null
end

# Apply dynamic completions to commands that need actor IDs
complete -c theater -n "__fish_seen_subcommand_from start stop state inspect message events" -a "(__theater_dynamic_completion)"
complete -c theater -n "__fish_seen_subcommand_from channel; and __fish_seen_subcommand_from open" -a "(__theater_dynamic_completion)"

# File completions for manifest files
complete -c theater -n "__fish_seen_subcommand_from build start" -a "*.toml" -d "Manifest file"
