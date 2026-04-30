---
name: python
description: Guidelines for working with Python code or tests. You **must** use this whenever you are writing, editing, or running Python code or tests.
---

## Source Code Guidelines

- Avoid nested functions.
- Avoid `global` and `nonlocal` variables.
- Avoid defining top-level "\_" prefixed private variables.
- Avoid `assert` in source code.
- Variables initialized in `__init__` should be private (`_` prefix) unless they are part of the public API.
- Instead of returning tuple, consider returning a typed object like a pydantic model.
- Use type annotations.
- Prefer `pathlib` over `os.path`.
- Prefer structured json logging over print statements.
- Prefer `pydantic` over alternatives like `dataclass`.
- Use `tenacity` instead of implementing your own retry logic.
- Prefer using official Python client libraries over custom HTTP or shell wrappers (e.g. `kubernetes` over `kubectl`).
- Avoid `Any` where possible.

## Testing

- Use functions for tests instead of classes.
- Prefer using public interfaces, otherwise add `# pylint: disable=protected-access`.
- Prefer parametrizing instead of writing multiple test functions for similar tests.
