---
name: implement
description: "Implement a piece of work based on a spec or set of tickets."
disable-model-invocation: true
---

Implement the work described by the user in the spec or tickets.

Use /tdd where possible, at pre-agreed seams.

Run typechecking regularly, single test files regularly, and the full test suite once at the end.

Commit the complete, tested layer to the current branch.

Once committed, use /code-review against the layer's actual base. If review
finds defects, fix them, rerun the affected checks, commit the fixes, and request
a focused re-review before stopping.
