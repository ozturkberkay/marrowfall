---
name: implement-design
description: You **must** use this when implementing the code for an approved design document.
---

**Must** be executed by the `engineer` subagent.

## Workflow

1. Read the entire design document end to end.
2. All tasks from the design document will be implemented as stacked PRs. For every task:
   a. Create branch `feature/{{design-title}}-{{n}}` rebased from the previous task branch
   or the latest `origin/main` if it's the first task.
   b. Implement the task using the guidelines below.
   c. Use `/code-review` skill to get feedback and iterate until it's perfect.
   d. **CRITICAL:** Ask for user's final approval. Iterate until approved.
   e. Create a PR with title: `[{{common_emoji}} {{design-title}} | Part {{n}}] {{task-name}}`.

## Guidelines

You **must** follow these guidelines at all times:

- Stick to the approved design document requirements. Do **not** deviate.
- Every line of code you write is a liability and **must** be justified.
    - You MUST ruthlessly optimize for simplicity, readability, and maintainability.
    - Avoid hacks, shortcuts, duplication, and speculative additions.
    - When introducing new paradigms and patterns, conform to researched best practices.
    - Proactively anticipate feedback and meticulously refactor and simplify like a perfectionist.
- Less is more. Simple is better.
- **NEVER** mix test with source code.
- Test directories should ALWAYS mirror the structure of the source code directories.
- Avoid writing single big files; prefer splitting into multiple.
- **NEVER** change the code coverage threshold!
- Never write code against a remembered API shape. Re-verify against
  the installed source first in case of training-data drift.
- Only disable linter rules if absolutely needed and keep the disable scope
  as narrow as possible. Your first instinct should be fixing the code!
- You **must** follow these first principles at all times:
    - Test Driven Development (TDD):
        - Red: Write a small test for new functionality that fails because the feature
          doesn't exist yet.
        - Green: Write just enough code to make that test pass.
        - Refactor: Clean up and improve the code and tests while ensuring the test
          stays green.
    - Three-Tier Testing Architecture:
        - Unit: Pure business logic tests with zero external dependencies
        - Integration: In-process integration tests with external dependencies
        - E2E: Black-box testing of the whole system based on user stories
    - Domain Driven Design (DDD):
        - Use entities, value objects, aggregates, repositories, and services to model
          complex domains.
        - Use ubiquitous language that is shared between developers and domain experts.
    - Functional Core, Imperative Shell (FCIS):
        - Functional core: Unit testable business logic
        - Imperative shell: External dependencies like I/O, use integration tests
    - Hexagonal Architecture
        - Core business logic is isolated from external systems, allowing easy swapping.
    - SOLID Principles
        - Single Responsibility Principle (SRP): A class should have one job or purpose.
        - Open-Closed Principle (OCP): New features should be added by adding new code, not
          modifying existing code.
        - Liskov Substitution Principle (LSP): Subclasses must behave in a way that doesn't
          break the functionality of the parent class.
        - Interface Segregation Principle (ISP): Create smaller, specific interfaces rather than
          one large, general-purpose one.
        - Dependency Inversion Principle (DIP): High-level modules should not depend on low-level
          modules; both should depend on abstractions (interfaces).
    - Clean Code
        - No duplication
        - Readable over clever
        - Comments only when necessary. Explain the why not the what. Make sure it's easy to
          understand for people with no context, keep it tiny.
        - Clear separation of concerns

## Validation

You **must** run this at the very end of an implementation pass, after all other
checks pass. Do not run it for incremental feedback.

```shell
# Make sure .gitignore is up-to-date! Stage everything (only at the end).
git add --all
# Run pre-commit ONCE; capture full output to /tmp/prek.log.
prek run --verbose > /tmp/prek.log 2>&1
echo "exit: $?"
# Inspect the log via grep, never re-run prek to see different slices.
```

Make sure the prek run is green and finish your work by going over this
checklist:

- [ ] If local development environment changes were made, relevant files like
      `README.md`, `includes.sh`, and `Brewfile` are updated.
- [ ] Files to ignore are added to `.gitignore`.
- [ ] If new environment variables are added, ensure config files for each
      environment are updated (if needed).
- [ ] There are no unit, integration, or end-to-end test gaps.

**!!!CRITICAL!!!** Your work is not done until every single pre-commit hook
passes, even the ones that are not relevant to your changes!
