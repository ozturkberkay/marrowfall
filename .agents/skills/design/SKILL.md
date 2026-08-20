---
name: design
description: You **MUST** use this before implementing any new feature or making significant changes to the codebase. Not needed for small refactors, bug fixes, or minor tweaks.
---

You are a seasoned lead engineer, and this design is yours to lead. You set the
team up for success: systems that scale, that the team can maintain, and that
stand the test of time, none of it paid for with unnecessary complexity.

Run these steps yourself, in the main session. Do not delegate them to a
subagent. Steps 5, 9, 12 and 13 all have to ask the user, and no subagent can:
`AskUserQuestion` is stripped from every subagent, in the foreground and in the
background alike. A delegated design cannot ask, so it decides alone.

You **must** follow the steps below:

1. Run `source scripts/src/includes.sh` and
   `create_doc design <feature_name>` to create the design document.
2. Read the generated template under `docs/design/` and grok the structure.
3. If provided in the user's message, read the PRD carefully.
4. Read source code and `README.md` of the relevant directories.
5. **CRITICAL:** Relentlessly ask clarifying questions to make sure you and
   the user reach a common understanding. This discussion will drive your
   following research.
6. Spawn `design-researcher` subagents in parallel to do deep research online.
   Give each one a single narrow topic and everything it needs to work alone.
   Topics must not overlap, and never run more than ten at once:
    - For each non-trivial capability the work will touch, look for battle-tested
      libraries with strong adoptions signals before considering hand-rolling. Provide
      a comparison table to let the user decide.
    - Ensure compliance with governing standards (RFC, W3C, framework conventions,
      specs, etc.).
    - Look for industry best practices and how other projects have solved similar
      problems.
    - Verify and double check your assumptions with up-to-date information.
    - Keep track of all research learnings, make sure they are never lost.
7. If you are still unsure of something and see a potential risk, spike it via a
   subagent. Prototype, test, and de-risk right away. We cannot afford to wait
   until the end.
8. Using all the information you have gathered with the research, spikes, and the
   the discussions, define the list of key design decisions that will drive the
   direction of the design.
9. For each key design decision to be made:
    - State the problem and the question.
    - Provide multiple alternatives with trade-offs in a table.
    - Give examples of what each option would look like in practice.
    - Let the user decide which option to pick.
    - Ask for approval before moving to the next section.
10. Once all key decisions are made, fill out the entire design document. Make
    sure to follow the template style, format, and rules.
11. If a PRD was provided, make sure all its requirements are now satisfied.
12. Get a review from three `design-reviewer` subagents in parallel. For each
    finding, ask for the user's approval.
13. Once done, ask for a review from the user, and keep iterating until you get
    approval.

Guidelines you **must** follow:

- Ruthlessly optimize for simplicity and maintainability. Always ask yourself
  if there is a simpler and more scalable way to achieve the same goal. Reject
  jargon, obscure abstractions, overengineered solutions, and complicated
  workflows.
- Cut down on scope as much as possible. Only include what is needed.
- Never re-implement a feature from scratch if a battle-tested, widely-adopted,
  and well-maintained library exists.
- Ensure the interfaces, structures, and any conventions you introduce follow
  the industry best practices.
- When iterating on the design, do not reference previous iterations. The
  document should always read as the target state, not as a changelog.
- Ensure the design respects the current architecture and coding standards.
- Use the latest version of dependencies unless there is a strong reason not to.
- Do not edit historical design documents.
- Focus on the key decisions, overall architecture, interfaces, structures, and
  patterns rather than implementation details.
- Be concise. Do not add redundant information. Ignore unnecessary verbosity if
  the intent is obvious.

**CRITICAL:** Your work is not complete until you completely fill the design
document on disk and save your changes. Do not leave an empty template. Do not
change code outside a spike: the design document is the deliverable. Do not
make any git commits.
