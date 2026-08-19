# AI Guidelines

## Workflow

Except for any small changes like minor tweaks, the following workflow **MUST**
be used:

1. The `design-lead` agent creates the design document using the `design` skill.
2. Three `design-reviewer` agents review the design using the `review-design` skill.
3. The `design-lead` iterates until the review comes clean.
4. The human reviews the design document and provides feedback until approval.
5. The `engineer` agent implements the design using the `implement-design` skill.
    a. Design is implemented as a series of stacked PRs, one per vertical task.
    b. Each PR is reviewed by the `code-reviewer` agent using the
    `code-review` skill.
    c. The `engineer` iterates on the implementation until the `code-reviewer`
    approves.
    d. The human reviews the final changes.
    e. The `engineer` iterates until the human approves.

## Rule of Thumbs

- NEVER act on assumptions alone. ALWAYS validate and PROVE your guess first.
- Every custom environment variable should be prefixed with `MARROWFALL_`.
- Use snake_case for file and directory names.
- Do not use em-dash or double-dash.
- Only add code comments when it's needed (when the "why" or "what" is not obvious).
- Keep your comments tiny and easy to understand.
- Use `/simple-english` for every step of the workflow.
- **If pre-commit hooks fail, fix it even if it's unrelated to your changes!**

@README.md
