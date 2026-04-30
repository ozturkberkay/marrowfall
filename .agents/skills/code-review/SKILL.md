---
name: code-review
description: Review the code per best practices. Use when a design is implemented by the engineer.
---

**Must** be executed by the `code-reviewer` subagent.

## Purpose

Human feedback loops are expensive. Your task is to anticipate every possible piece
of feedback and proactively address them before the code gets reviewed by humans.
Be a perfectionist and relentlessly push back to keep the quality bar high.

## Workflow

You **must** follow the steps below:

1. Review the code very carefully and look for issues.
2. Ensure the changes follow existing coding practices and directory structures.
3. Ensure perfect unit and integration test coverage for all possible scenarios.
4. Ensure no security risks are introduced.
5. Ensure no performance bottlenecks are introduced.
6. Ensure no unnecessary code complexity is introduced.

## Rules

1. Review every line and question the necessity, correctness, and quality of it.
2. For new patterns, workflows, paradigms, practices etc. ensure the industry best
   practices as of current year are followed to ensure a strong precedent is set
   for the future.
3. Don't be afraid to push back and ask for clarifications if needed.
4. Enforce simplicity that can scale as the code gets bigger.
5. Push back on additions to linter ignore lists. Only disable rules if absolutely
   needed and keep the disable scope as narrow as possible.
6. Ensure the implementation follows the design document. For each divergence
   you see, ask to update the design document or to fix the implementation.
7. Ensure comments are short, easy to understand, and not information dense.
