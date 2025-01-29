# Making Changes to Theater

This document outlines the process for making changes to the Theater project. We use a structured approach to document our changes, making it easier for team members to understand what's happening and why.

## Change Process

1. **Create a Proposal**
   - All significant changes start with a proposal
   - Create a new markdown file in `changes/proposals/`
   - Use the format: `YYYY-MM-DD-brief-description.md`
   - Follow the proposal template structure (see below)

2. **Update In-Progress List**
   - Add your proposal to `changes/in-progress.md`
   - Include the proposal file name and a brief description

3. **Work on the Change**
   - Document your progress in the "Working Notes" section
   - Update as you encounter challenges or make decisions
   - Keep notes about what worked and what didn't

4. **Complete the Change**
   - Fill out the "Final Notes" section
   - Document the final implementation details
   - Note any future considerations or follow-up work
   - Update `in-progress.md` to mark as complete

## Proposal Template

```markdown
# [Brief Description of Change]

## Description
- What is being changed
- Why this change is necessary
- Expected benefits and potential risks
- Any alternatives considered

## Working Notes
- Ongoing notes about the implementation
- Challenges encountered
- Decisions made and their reasoning
- References to relevant commits or discussions

## Final Notes
- Final implementation details
- What was actually changed
- Any deviations from the original plan
- Lessons learned
- Future considerations
```

## Example

See `changes/proposals/2025-01-29-random-id-system.md` for an example of a completed change proposal.

## Tips for Good Change Documentation

1. Be explicit about your reasoning
2. Document both successful and unsuccessful approaches
3. Include relevant code examples or diagrams
4. Reference related issues or discussions
5. Update regularly during the change process

## Benefits

- Makes project evolution more transparent
- Helps new team members understand the codebase
- Creates a knowledge base for future reference
- Facilitates code review and discussion
- Provides context for future changes