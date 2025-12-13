# Theater Project Changes

This directory tracks proposed and in-progress changes to the Theater project at the workspace level.

## Directory Structure

```
changes/
├── proposals/          # Detailed change proposals
│   └── 2025-11-30-handler-migration.md
├── in-progress/        # Active work tracking
│   └── handler-migration.md
└── README.md          # This file
```

## Change Process

1. **Proposal**: Create a detailed proposal in `proposals/` describing the change, motivation, design, and implementation plan
2. **In Progress**: Track active work in `in-progress/` with detailed status updates
3. **Completion**: Update the in-progress document with completion notes and learnings

## Active Changes

| Date | Change | Status | Tracking |
|------|--------|--------|----------|
| 2025-11-30 | Handler Migration | In Progress | [handler-migration.md](in-progress/handler-migration.md) |

## Completed Changes

None yet at the workspace level.

## Note on Crate-Level Changes

Individual crates (like `theater`, `theater-client`, etc.) may have their own `changes/` directories for tracking crate-specific changes. This top-level directory is for workspace-wide changes that affect multiple crates or the overall project structure.
