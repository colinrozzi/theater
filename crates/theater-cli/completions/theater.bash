#!/bin/bash

# Enhanced bash completion for theater CLI with dynamic completions

_theater_completion() {
    local cur prev words cword
    _init_completion || return

    # Use the dynamic completion system for actor IDs and other runtime data
    case "${words[1]}" in
        start|stop|state|inspect|message|events)
            if [[ $cword -eq 2 ]]; then
                # Get dynamic completions from the theater CLI
                local completions
                completions=$(theater dynamic-completion "${COMP_LINE}" "${cur}" 2>/dev/null)
                if [[ $? -eq 0 && -n "$completions" ]]; then
                    COMPREPLY=($(compgen -W "$completions" -- "$cur"))
                    return 0
                fi
            fi
            ;;
        channel)
            if [[ $cword -eq 2 ]]; then
                COMPREPLY=($(compgen -W "open" -- "$cur"))
                return 0
            elif [[ $cword -eq 3 && "${words[2]}" == "open" ]]; then
                # Get actor IDs for channel open
                local completions
                completions=$(theater dynamic-completion "${COMP_LINE}" "${cur}" 2>/dev/null)
                if [[ $? -eq 0 && -n "$completions" ]]; then
                    COMPREPLY=($(compgen -W "$completions" -- "$cur"))
                    return 0
                fi
            fi
            ;;
        create)
            if [[ $cword -eq 2 ]]; then
                COMPREPLY=($(compgen -W "basic http" -- "$cur"))
                return 0
            fi
            ;;
        completion)
            if [[ $cword -eq 2 ]]; then
                COMPREPLY=($(compgen -W "bash zsh fish powershell elvish" -- "$cur"))
                return 0
            fi
            ;;
    esac

    # Default completion for first argument (commands)
    if [[ $cword -eq 1 ]]; then
        local commands="build channel completion create events inspect list list-stored message start state stop subscribe"
        COMPREPLY=($(compgen -W "$commands" -- "$cur"))
        return 0
    fi

    # File completion for manifest files
    case "${words[1]}" in
        start|build)
            if [[ $cword -eq 2 ]]; then
                COMPREPLY=($(compgen -f -X '!*.toml' -- "$cur"))
                return 0
            fi
            ;;
    esac

    # Default to no completion
    COMPREPLY=()
}

# Register the completion function
complete -F _theater_completion theater
