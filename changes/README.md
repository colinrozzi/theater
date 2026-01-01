# Theater Project Changes

This directory tracks proposed and in-progress changes to the Theater project at the workspace level.

## Directory Structure

```
changes/
├── proposals/          # Detailed change proposals
│   └── 2025-11-30-handler-migration.md
├── in-progress/        # Active work tracking
│   ├── handler-migration.md           # Handler crate extraction (COMPLETE)
│   ├── handler-test-infrastructure.md # Test actors + integration tests (ACTIVE)
│   ├── message-router-architecture.md # Message routing design
│   └── ...                            # Other migration tracking docs
└── README.md          # This file
```

## Change Process

1. **Proposal**: Create a detailed proposal in `proposals/` describing the change, motivation, design, and implementation plan
2. **In Progress**: Track active work in `in-progress/` with detailed status updates
3. **Completion**: Update the in-progress document with completion notes and learnings

## Active Changes

| Date | Change | Status | Tracking |
|------|--------|--------|----------|
| 2025-12-31 | Handler Test Infrastructure | In Progress | [handler-test-infrastructure.md](in-progress/handler-test-infrastructure.md) |

## Completed Changes

| Date | Change | Status | Tracking |
|------|--------|--------|----------|
| 2025-11-30 | Handler Migration | Complete (100%) | [handler-migration.md](in-progress/handler-migration.md) |

## Note on Crate-Level Changes

Individual crates (like `theater`, `theater-client`, etc.) may have their own `changes/` directories for tracking crate-specific changes. This top-level directory is for workspace-wide changes that affect multiple crates or the overall project structure.
