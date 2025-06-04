#compdef theater

# Enhanced zsh completion for theater CLI with dynamic completions

_theater() {
    local context state line
    typeset -A opt_args

    _arguments -C \
        '(-v --verbose)'{-v,--verbose}'[Turn on verbose output]' \
        '(--json)--json[Display output in JSON format]' \
        '1: :_theater_commands' \
        '*: :->args'

    case $state in
        args)
            case $words[2] in
                start|stop|state|inspect|message|events)
                    _theater_dynamic_completion
                    ;;
                channel)
                    case $words[3] in
                        open)
                            _theater_dynamic_completion
                            ;;
                        *)
                            _values 'channel commands' 'open'
                            ;;
                    esac
                    ;;
                create)
                    _values 'templates' 'basic' 'http'
                    ;;
                completion)
                    _values 'shells' 'bash' 'zsh' 'fish' 'powershell' 'elvish'
                    ;;
                build)
                    _files -g '*.toml'
                    ;;
                list|list-stored|subscribe)
                    # These commands don't need additional arguments
                    ;;
            esac
            ;;
    esac
}

_theater_commands() {
    local commands
    commands=(
        'build:Build a Theater actor to WebAssembly'
        'channel:Channel operations'
        'completion:Generate shell completion scripts'
        'create:Create a new Theater actor project'
        'events:Get actor events'
        'inspect:Inspect a running actor'
        'list:List all running actors'
        'list-stored:List stored actor IDs'
        'message:Send a message to an actor'
        'start:Start or deploy an actor from a manifest'
        'state:Get actor state'
        'stop:Stop a running actor'
        'subscribe:Subscribe to real-time events from an actor'
    )
    _describe 'commands' commands
}

_theater_dynamic_completion() {
    local completions
    local line_words
    line_words=${words[*]}
    
    # Get dynamic completions from theater CLI
    completions=(${(f)"$(theater dynamic-completion "$line_words" "${words[-1]}" 2>/dev/null)"})
    
    if (( ${#completions[@]} > 0 )); then
        _describe 'actors' completions
    else
        # Fallback to file completion
        _files
    fi
}

_theater "$@"
