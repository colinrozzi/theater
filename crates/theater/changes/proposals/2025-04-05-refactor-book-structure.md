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

- **Tasks:**
    - [x] Create the new "Architecture" top-level section in SUMMARY.md
    - [x] Move and potentially rename architecture.md to the new section
    - [x] Reorganize core-concepts files around the three pillars
    - [x] Create new files as needed for missing content
    - [x] Update cross-references in all affected files
    - [x] Review the flow and readability of the revised structure
    - [x] Ensure proper navigation between sections

- **Decisions:**
    - *Decision made:* Core concept files named after the three pillars: wasm-components.md, actor-model.md, traceability.md
    - *Decision made:* Architecture section includes: overview.md, components.md, data-flow.md, implementation.md
    - *Decision made:* Created comprehensive content for all new sections

- **Challenges:**
    - Maintaining a balance between conceptual explanations and technical details
    - Ensuring consistency in terminology across all sections
    - Avoiding duplication while ensuring each section can stand on its own

## Final Notes

- **Final implementation details:**
    - Created a new Architecture section with 4 pages (README.md, overview.md, components.md, data-flow.md, implementation.md)
    - Created 3 new core concept pages focused on the pillars (wasm-components.md, actor-model.md, traceability.md)
    - Updated introduction pages to align with the new structure
    - Updated the main README.md to reflect the three pillars approach
    - Updated SUMMARY.md with the new navigation structure

- **Deviations from original plan:**
    - We didn't include a separate "Getting Started" section yet, leaving this for a future enhancement
    - Core concepts retained the existing detailed pages (actor-ids.md, state-management.md, etc.) to maintain existing content while adding the pillar-focused pages

- **Lessons learned:**
    - A clear conceptual structure makes it easier to organize detailed content
    - The three pillars approach provides a strong narrative thread throughout the documentation
    - Separating "what" from "how" makes the material more accessible to different audience types

- **Future considerations:**
    - Add a dedicated "Getting Started" section with quick examples
    - Review and potentially consolidate detailed pages in Core Concepts
    - Add more diagrams to illustrate concepts and architecture
    - Consider a glossary for key Theater terminology
