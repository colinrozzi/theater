# theater-server-cli

> [!NOTE]
> This documentation is incomplete, please reach out to me at colinrozzi@gmail.com I very much appreciate your interest and would love to hear from you!

Standalone CLI for Theater server management.

## Overview

`theater-server-cli` provides a dedicated command-line interface for running and managing Theater servers. This is a lightweight alternative to the full `theater-cli` when you only need server functionality.

## Installation

```bash
cargo install theater-server-cli
```

## Usage

```bash
# Start a Theater server
theater-server start

# Start a Theater server and log to stdout
theater-server start --log-stdout

# Start a Theater server with debug logging
theater-server start --log-level debug
```

## License

This project is licensed under the Apache License 2.0 - see the [LICENSE](../../LICENSE) file for details.
