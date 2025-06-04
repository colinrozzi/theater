#!/bin/bash

# Test script for Theater CLI shell completion system

echo "=== Testing Theater CLI Shell Completion System ==="
echo

# Check if theater binary exists
if ! command -v theater &> /dev/null; then
    echo "❌ Theater CLI binary not found in PATH"
    echo "   Please build and install the binary first"
    exit 1
fi

echo "✅ Theater CLI binary found"

# Test basic completion generation
echo
echo "=== Testing Basic Completion Generation ==="

for shell in bash zsh fish powershell elvish; do
    echo -n "Testing $shell completion generation... "
    if theater completion $shell > /dev/null 2>&1; then
        echo "✅"
    else
        echo "❌"
    fi
done

# Test dynamic completion (this requires the server to be running)
echo
echo "=== Testing Dynamic Completion ==="

echo -n "Testing dynamic completion (may fail if server not running)... "
if theater dynamic-completion "theater stop " "test" > /dev/null 2>&1; then
    echo "✅"
else
    echo "⚠️  (Server not running or other issue - this is expected)"
fi

# Test completion file generation
echo
echo "=== Testing Completion File Generation ==="

temp_dir=$(mktemp -d)
echo "Using temporary directory: $temp_dir"

for shell in bash zsh fish; do
    output_file="$temp_dir/theater.$shell"
    echo -n "Generating $shell completion to file... "
    if theater completion $shell --output "$output_file" > /dev/null 2>&1; then
        if [ -f "$output_file" ]; then
            echo "✅ ($(wc -l < "$output_file") lines)"
        else
            echo "❌ File not created"
        fi
    else
        echo "❌ Generation failed"
    fi
done

# Test that enhanced completion scripts exist
echo
echo "=== Testing Enhanced Completion Scripts ==="

completion_dir="completions"
for shell in bash zsh fish; do
    script_file="$completion_dir/theater.$shell"
    echo -n "Checking enhanced $shell script... "
    if [ -f "$script_file" ]; then
        echo "✅ ($(wc -l < "$script_file") lines)"
    else
        echo "❌ Not found"
    fi
done

# Cleanup
rm -rf "$temp_dir"

echo
echo "=== Shell Completion Testing Complete ==="
echo
echo "🎉 To install completions:"
echo "   bash:  theater completion bash > ~/.local/share/bash-completion/completions/theater"
echo "   zsh:   theater completion zsh > ~/.local/share/zsh/site-functions/_theater"
echo "   fish:  theater completion fish > ~/.config/fish/completions/theater.fish"
echo
echo "📖 See completions/README.md for detailed installation instructions"
