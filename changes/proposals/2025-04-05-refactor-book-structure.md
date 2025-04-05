# Book Structure Refinement Proposal

## Overview

This proposal aims to refine the Theater documentation structure to better serve both newcomers and experienced users by creating a clearer separation between conceptual understanding and technical implementation.

## Motivation

The current book organization mixes high-level concepts with detailed implementation specifics, which can create confusion for users trying to grasp Theater's fundamental ideas. By restructuring the documentation to follow a more natural learning progression, we can improve comprehension and user experience.

## Proposed Changes

### 1. Refocus Core Concepts Around Three Pillars

Reorganize the "Core Concepts" section to explicitly highlight Theater's three foundational pillars:

**WebAssembly Components & Sandboxing**
- Security boundaries and capabilities
- Deterministic execution
- Interface definitions
- Language-agnostic components

**Actor Model & Supervision**
- Actor lifecycle and isolation
- Message-passing communication
- Hierarchical supervision
- Fault tolerance strategies

**Traceability & Verification**
- Event Chain system
- Deterministic replay
- State management
- Debugging and inspection

### 2. Create a New "Architecture" Section

Move detailed implementation specifics from "Core Concepts" to a new top-level "Architecture" section that explains:
- Component relationships
- Internal design decisions
- Data flow between subsystems
- Implementation details

### 3. Logical Reading Path

Structure the book to follow a natural progression:
1. **Introduction**: Why Theater exists (problem space)
2. **Getting Started**: Quick practical examples
3. **Core Concepts**: What Theater is (fundamental ideas)
4. **Architecture**: How Theater works (implementation)
5. **User Guide**: How to use Theater (practical usage)
6. **Development**: How to extend Theater (building on it)

## Benefits

1. **Clearer Mental Model**: Readers develop a solid conceptual understanding before encountering implementation details.

2. **Targeted Information**: Different audiences can more easily find the content relevant to their needs:
   - New users can focus on concepts and getting started
   - Users can reference the user guide for day-to-day operations
   - Contributors can dive into architecture for implementation details

3. **Reduced Cognitive Load**: By separating "what" from "how," readers can build a mental model without being overwhelmed by technical specifics.

4. **Improved Documentation Maintainability**: Clearer separation makes it easier to update either conceptual or implementation documentation as the project evolves.

## Implementation Plan

1. Create new architecture section with current architecture.md as its foundation
2. Restructure core-concepts around the three pillars
3. Update cross-references and navigation
4. Review content flow and continuity

## Conclusion

This restructuring will enhance the Theater documentation's effectiveness by providing a more intuitive learning path while maintaining access to detailed technical information for those who need it.

## Working Notes
*(This section to be filled in during the implementation)*

- **Tasks:**
    - [ ] Create the new "Architecture" top-level section in SUMMARY.md
    - [ ] Move and potentially rename architecture.md to the new section
    - [ ] Reorganize core-concepts files around the three pillars
    - [ ] Create new files as needed for missing content
    - [ ] Update cross-references in all affected files
    - [ ] Review the flow and readability of the revised structure
    - [ ] Ensure proper navigation between sections

- **Decisions:**
    - *Decision needed:* Final names for core concept files
    - *Decision needed:* Structure of the Architecture section (subsections?)
    - *Decision needed:* Additional content needed for any section

- **Challenges:**
    - *(Track any issues encountered here)*

## Final Notes
*(This section to be filled in upon completion)*

- **Final implementation details:**
    - *(Summary of what was actually done)*
- **Deviations from original plan:**
    - *(Note any changes made during implementation)*
- **Lessons learned:**
    - *(Any insights gained)*
- **Future considerations:**
    - *(Any follow-up tasks or related improvements)*
