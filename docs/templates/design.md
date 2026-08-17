# Design: {{name}}

Be concise, but clear for future readers with no context.
Ignore unnecessary verbosity if the intent is obvious.
Avoid information dense paragraphs. Prefer short bullet lists.
1,000 lines limit.
Do not introduce new sections.

## Context & Problem

> **Definition:** What we are solving and why now. One short paragraph that
> a future engineer can read cold.
> **Rule:** Reference the brief, PRD, or issue if one exists. State the user,
> system, or business problem in plain language. Do not describe the solution
> here. Describe the problem only in one paragraph.

## In Scope

> **Definition:** The capabilities this design delivers.
> **Rule:** If removing an item does not break a task, it does not belong here.
> Short, easy to understand entries for overall capabilities. Do not list
> implementation details. One sentence per entry. Focus on overall deliverables,
> not the task details.

## Out of Scope

> **Definition:** Things this design deliberately does NOT do.
> **Rule:** Each entry names the excluded capability. Speculative additions
> that have no current consumer in the Tasks section belong here. Short, easy
> to understand entries for overall non-capabilities. Do not list implementation
> details. One sentence per entry.

## Terminology

> **Definition:** Any new or domain-specific terms introduced by this design.
> **Rule:** Should respect the existing terminology used in the codebase. Align
> on a ubiquitous language. Short list and one sentence definitions. Only include
> terms that may not be immediately clear to a future reader.

## Key Decisions

> **Definition:** Every architectural choice that drives this design. The
> reader of this section alone should understand why the design looks the way
> it does.
> **Rule:** Each decision is a question with at least two options. Each option has
> pros, cons, and a rationale for why it was accepted or rejected. Put an example
> snippet for each option.
> **IMPORTANT:** This section is the most critical part, especially for future
> human readers. The options must be clearly explained with enough context and must
> demonstrate how they would look like in practice with example snippets. Options
> may not be obvious for the readers.

### {{key design question 1}}

#### ✅ Option 1: {{short option name}}

Description of the option and a small example of what it looks like in practice.
Descriptions should be clear enough for future readers with no context.

**Pros:**

- ...

**Cons:**

- ...

**Rationale:**

#### ❌ Option 2: {{short option name}}

Rationale should include why this option was rejected.

## Architecture Overview

> **Definition:** High-level system design, components, and their
> interactions.
> **Rule:** Must be understandable without implementation details. Use diagrams.

## Third Party Dependencies

> **Definition:** Every non-trivial third-party capability used by this
> design.
> **Rule:** Prefer using widely adopted libraries rather than re-inventing
> the wheel. Break down all the alternatives considered in a table format.

## Structure

> **Definition:** The file, directory, and identifier (e.g. classes) layout of the
> implementation.
> **Rule:** Must be consistent with the existing codebase structure.

## Specs & Standards

> **Definition:** RFCs, specifications, standards, protocols, conventions,
> guidelines, and community best practices this design conforms to.
> **Rule:** Design must be proven to comply with relevant standards. Every
> wire format cites its governing standard, including the section or clause.

## Interfaces

> **Definition:** External or internal endpoints, contracts, and protocols.
> **Rule:** Every contract is documented. Each wire-format element cites its
> entry in Specs & Standards. No assumptions about caller behavior; spell
> them out.

## Existing Code & Reuse

> **Definition:** Audit of what already exists in the repo that this design
> touches, extends, or reuses.
> **Rule:** Catch "this duplicates X that already exists" before implementation.

## Logic

> **Definition:** Core logic or processing required to implement the capability.
> **Rule:** Use code snippets showing only the most important parts of the logic.

## Edge Cases & Constraints

> **Definition:** Special cases, limits, or environmental considerations.
> **Rule:** Cover important things that could break the implementation if ignored.
> Reference Standards & Specs where the edge case is governed by an external
> rule (e.g., size limit from an RFC, browser quirk from a W3C note). Keep it
> short. One sentence per entry if possible.

## Test Plan

> **Definition:** How the capability will be tested and validated.
> **Rule:** Include unit, integration, and e2e tests tied to the PRD success
> criteria. Cross-reference Out of Scope so we do not accidentally test future
> capability.

## Documentation Changes

> **Definition:** Updates needed for `README.md` and any subdirectory
> documentation.
> **Rule:** Cover setup, usage, and any new concepts introduced.

## Development Environment Changes

> **Definition:** Modifications to `Brewfile`, setup scripts, environment
> variables, container layout, etc.
> **Rule:** Ensure every new engineer can onboard smoothly.

## Tasks

> **Definition:** Bite-sized, vertical slices of work that can be independently
> implemented, tested, and reviewed.
> **Rule:** Each task has a clearly defined scope, strict requirements, and
> success criteria. Tasks are vertical to get quick feedback. Use a DAG of
> tasks in addition to the table below to show sequential and parallel work.

| #   | Task Name | Task Description | Success Criteria | Dependencies |
| --- | --------- | ---------------- | ---------------- | ------------ |
