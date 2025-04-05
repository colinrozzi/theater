# Refactor Book Structure for Core Concepts and Architecture

## Description

- **What is being changed:**
    - Reorganize the Theater book structure, specifically the `core-concepts` section and the placement of the detailed architecture documentation.
    - The `core-concepts` section will be refocused to cover the three fundamental pillars identified in the project introduction:
        1. WebAssembly Components (Sandboxing, Determinism, Interfaces)
        2. Actor Model with Supervision (Isolation, Communication, Recovery)
        3. Complete Traceability (Event Chain, Debugging, Verification)
    - The detailed technical architecture documentation (currently residing in `core-concepts/architecture.md`) will be moved to a new, dedicated top-level section, likely named "Architecture" or "System Internals", placed logically after "Core Concepts".
    - The file `core-concepts/architecture.md` will be moved and potentially renamed (e.g., `component-overview.md`) within the new section.

- **Why this change is necessary:**
    - The current structure mixes high-level conceptual explanations with low-level technical architecture details within the `core-concepts` section.
    - This can be confusing for new users trying to grasp the fundamental ideas of Theater first.
    - Aligning the book structure more closely with the introductory material ("Why Theater?") provides a clearer learning path: Introduction (Why) -> Core Concepts (What) -> Architecture (How).

- **Expected benefits:**
    - Improved onboarding experience for programmers new to Theater.
    - Clearer separation between fundamental concepts and implementation details.
    - More logical flow and easier navigation through the book.
    - Better alignment between the book's structure and the project's stated goals and principles.

- **Potential risks:**
    - Requires careful updating of `SUMMARY.md` to reflect the new structure.
    - Internal cross-references within markdown files might need updating to point to the new locations.
    - Minor risk of temporarily broken links if updates are not managed carefully during implementation.

- **Alternatives considered:**
    - Considered simply reorganizing the content *within* the existing `core-concepts` section, but decided that separating the detailed architecture into its own top-level section provides superior clarity and structure.

## Working Notes
*(This section to be filled in during the implementation)*

- **Tasks:**
    - [ ] Finalize the exact name for the new top-level section ("Architecture", "System Internals", etc.).
    - [ ] Identify which existing files will be used/merged/refined for each of the three pillars in `core-concepts`.
    - [ ] Draft the content/structure for the updated `core-concepts` pages.
    - [ ] Move and potentially rename `architecture.md` to the new section.
    - [ ] Update `SUMMARY.md` to reflect the new book structure.
    - [ ] Review all affected markdown files for broken cross-references and update as needed.
    - [ ] Read through the modified sections to ensure flow and clarity.

- **Decisions:**
    - *Decision needed:* Exact name for the new section. (Leaning towards "Architecture" for simplicity).
    - *Decision needed:* Final filenames within the refocused `core-concepts`.

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
