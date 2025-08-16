# Theater CLI Templates

This directory contains templates for creating new Theater actor projects using the `theater create` command.

## Template Structure

Each template is organized as a directory with the following structure:

```
templates/
├── basic/
│   ├── template.toml    # Template metadata
│   ├── Cargo.toml       # Template files...
│   ├── manifest.toml
│   ├── world.wit
│   ├── lib.rs
│   ├── README.md
│   └── wkg.toml
├── message-server/
│   └── ...
└── supervisor/
    └── ...
```

## Template Metadata Format

Each template directory must contain a `template.toml` file with the following format:

```toml
[template]
name = "template-name"
description = "Description of what this template provides"

[files]
"target/path/file.ext" = "source-file.ext"
"Cargo.toml" = "Cargo.toml"
"src/lib.rs" = "lib.rs"
# ... more file mappings
```

- **name**: The template identifier (should match the directory name)
- **description**: Human-readable description shown in `theater create --help`
- **files**: Maps target paths in the created project to source files in the template directory

## Template Variables

Templates use Handlebars templating with the following variables:

- `{{project_name}}` - The project name as provided by the user
- `{{project_name_snake}}` - Project name with dashes converted to underscores (for Rust identifiers)

You can also use the `default` helper:
```handlebars
{{default some_optional_var "fallback_value"}}
```

## Available Templates

### basic
A simple Theater actor with basic functionality. Implements only the core `theater:simple/actor` interface with state management and initialization logging.

### message-server  
A Theater actor with message server capabilities. Implements `theater:simple/actor` + `theater:simple/message-server-client` for handling direct messages, request/response patterns, and channels.

### supervisor
A Theater actor with supervisor capabilities for managing child actors. Implements `theater:simple/actor` + `theater:simple/supervisor-handlers` with error handling and restart strategies.

## Usage

```bash
# List available templates
theater create --help

# Create a project from a template
theater create my-actor --template basic
theater create my-server --template message-server  
theater create my-supervisor --template supervisor
```

## Creating New Templates

1. Create a new directory under `templates/`
2. Add a `template.toml` file with metadata
3. Add template files using Handlebars syntax for variables
4. The template will be automatically discovered by the CLI

## Migration from Code-Based Templates

This system replaces the previous approach where templates were hardcoded as strings in the Rust code. The new system provides:

- **Better maintainability** - Templates are separate files that are easier to edit
- **Handlebars templating** - More powerful variable substitution 
- **Automatic discovery** - New templates are found automatically
- **Clear separation** - Template content is separated from CLI logic
