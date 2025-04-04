# Troubleshooting Guide

> **Note:** This guide is currently under development and will be expanded with more troubleshooting scenarios soon.

## Common Issues

### Installation Problems

#### Nix Environment Issues

**Problem:** Error when running `nix develop` about experimental features.

**Solution:** Make sure flakes are enabled in your Nix configuration:
```bash
# Add to /etc/nix/nix.conf:
experimental-features = nix-command flakes

# Or use environment variable:
export NIX_CONFIG="experimental-features = nix-command flakes"
```

#### Build Failures

**Problem:** Cargo build fails with linking errors.

**Solution:** Ensure you have the required system dependencies:
```bash
# For Debian/Ubuntu
sudo apt install build-essential llvm clang libssl-dev pkg-config

# For macOS with Homebrew
brew install llvm openssl pkg-config
```

### Runtime Issues

#### Actor Initialization Failures

**Problem:** Actor fails to initialize with a generic error.

**Solution:**
1. Check your manifest file for syntax errors
2. Ensure the component path is correct and the WASM file exists
3. Verify that your actor implements the required interfaces

#### Message Routing Problems

**Problem:** Messages aren't reaching the intended actor.

**Solution:**
1. Check handler configuration in the actor manifest
2. Verify port numbers and network settings
3. Ensure the message format matches what the actor expects
4. Check logs for any error messages or warnings

### Hash Chain Verification

**Problem:** Hash chain verification is failing.

**Solution:**
1. Ensure no one has manually modified the state
2. Check for version mismatches in the components
3. Verify that all state transitions are using the proper APIs
4. Run with verbose logging to identify the exact point of failure

## Debugging Techniques

### Verbose Logging

Enable detailed logging to troubleshoot issues:

```bash
# Set environment variable before running
export THEATER_LOG=debug

# Then run your command
cargo run -- --manifest your_manifest.toml
```

### State Inspection

Inspect actor state using the Theater CLI:

```bash
# Dump current state
cargo run -- inspect --actor-id <actor-id> --state

# View state history
cargo run -- inspect --actor-id <actor-id> --history

# Verify hash chain
cargo run -- verify --actor-id <actor-id>
```

### Network Debugging

For network-related issues, use standard tools:

```bash
# Check if port is in use
lsof -i :<port>

# Test HTTP endpoint
curl -v http://localhost:<port>/path

# Monitor network traffic
tcpdump -i lo0 port <port>
```

## Getting Help

If you're still experiencing issues:

1. Check existing [GitHub issues](https://github.com/colinrozzi/theater/issues)
2. Search the documentation for similar problems
3. Open a new issue with detailed reproduction steps and logs
4. Reach out on the project's discussion forums
