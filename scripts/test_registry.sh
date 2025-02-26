#!/bin/bash
# Test script for the theater registry integration

set -e  # Exit on any error

echo "=== Testing Theater Registry Integration ==="

# Create a test registry
TEST_REGISTRY="$HOME/.theater/test-registry"
echo "Creating test registry at $TEST_REGISTRY"
mkdir -p "$TEST_REGISTRY"

# Initialize the registry
echo "Initializing registry..."
cargo run --bin theater-cli -- registry init "$TEST_REGISTRY"

# Register the chat actor
ACTOR_PATH="/Users/colinrozzi/work/actors/chat"
echo "Registering actor from $ACTOR_PATH"
cargo run --bin theater-cli -- registry register "$ACTOR_PATH" --registry "$TEST_REGISTRY"

# List actors in the registry
echo "Listing actors in registry:"
cargo run --bin theater-cli -- registry list --path "$TEST_REGISTRY"

# Set the registry path for the test
export THEATER_REGISTRY="$TEST_REGISTRY"

# Try to start the actor by name
echo "Starting actor by name..."
cargo run --bin theater-cli -- actor start chat

# Try to start with specific version (assuming version 0.1.0 from your actor.toml)
echo "Starting actor with specific version..."
cargo run --bin theater-cli -- actor start chat:0.1.0

echo "=== Test completed! ==="
