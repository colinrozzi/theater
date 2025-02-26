#!/bin/bash
# Test script for the Theater registry integration

set -e  # Exit on any error

echo "=== Testing Theater Registry Integration ==="

# Create a test registry
TEST_REGISTRY="$HOME/theater-test-registry"
echo "Creating test registry at $TEST_REGISTRY"
mkdir -p "$TEST_REGISTRY"

# Initialize the registry
echo "Initializing registry..."
theater registry init "$TEST_REGISTRY"

# Register an actor
ACTOR_PATH="/Users/colinrozzi/work/actors/chat"
echo "Registering actor from $ACTOR_PATH"
theater registry register "$ACTOR_PATH" --registry "$TEST_REGISTRY"

# List actors in the registry
echo "Listing actors in registry:"
theater registry list --path "$TEST_REGISTRY"

# Set the registry path for the test
export THEATER_REGISTRY="$TEST_REGISTRY"

# Try to start the actor by name
echo "Starting actor by name..."
theater start chat

# Try to start with specific version (assuming version 0.1.0 from your actor.toml)
echo "Starting actor with specific version..."
theater start chat:0.1.0

echo "=== Test completed! ==="
