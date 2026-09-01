# Delivery workflow

Code reaches `main` through pull requests. Direct pushes to `main` are outside
this workflow because they discard the review record that binds an issue,
diff, checks, and merge.

## Units

- **Issue**: one accepted outcome or implementation milestone.
- **Stack**: 2–4 genuinely dependent PR layers for one issue.
- **PR layer**: one complete, observable, tested contract. It targets the layer
  below; the bottom layer targets `main`.
- **Commit**: local implementation history inside one layer.

Use a stack only when each upper layer requires the lower layer. Independent
work stays unstacked. A stack never spans roadmap issues, and a dependent issue
does not start until the stack for its blocker has merged.

## Per issue

1. Branch the bottom layer from `main` as
   `<type>/<issue>-<layer-slug>`. Branch each upper layer from the branch below
   it. For a stack, initialize or link the chain with `gh stack` so every base
   remains explicit.
2. Implement one observable layer with the `implement` skill. Use the crate's
   focused test loop while working; run `scripts/check.sh` when the layer is
   complete. Commit with Conventional Commits.
3. Run `/code-review` against the layer's actual base. If it finds a verified
   defect, fix it, rerun the affected checks and `scripts/check.sh`, commit the
   fix, and request a focused re-review. The layer is review-complete only when
   its current `HEAD` is the reviewed `HEAD`.
4. Submit a stack with `gh stack submit --auto --open`; push an unstacked branch
   and open its PR with `gh pr create --body-file <path>`. Include
   `Part of #<issue>` in every lower stacked PR body and `Closes #<issue>` only
   in the cumulative top (or sole unstacked PR).
5. Review the exact cumulative top head with one independent adversarial
   reviewer. Add a specialist adversarial review for a layer that changes
   identity, authority, persistence, gate mediation, sandboxing, connector
   egress, audit, or recovery.
6. Record the base commit SHA, reviewed top-head SHA, and
   `git rev-parse <top-branch>^{tree}` beside the cumulative review evidence.
7. Address cumulative findings once. Rerun the affected checks and
   `scripts/check.sh`, commit the fixes, then request a focused re-review of the
   findings and fixes. Replace the recorded head SHA and tree with the reviewed
   fixed state. Resolve new adjacent questions by deterministic diagnosis or
   explicit rescoping; do not start recursive general-review waves.
8. If `main`, a lower layer, or any PR base changes, run `gh stack sync`, rerun
   `scripts/check.sh` on every affected layer, and repeat each review whose diff
   or cumulative tree changed. Changed evidence is invalid evidence.
9. Immediately before merge, establish a merge freeze: no stack pushes or
   other `main` merges until verification completes. Confirm the PR or stack
   still uses the reviewed base, head SHA, and top tree. For a stack, submit the
   atomic async merge with `PUT repos/<owner>/<repo>/pulls/<top-pr>/merge-async`
   through `gh api`, passing `sha=<reviewed-head-sha>`,
   `merge_method=squash`, and `merge_action=direct_merge`, then poll its UUID to
   a terminal result. For an unstacked PR, run `gh pr merge <pr> --squash
   --match-head-commit <reviewed-head-sha>`. A SHA mismatch or failed merge
   returns to step 8. After success, verify that `main` has the recorded top
   tree, then release the freeze.

## Boundaries

- Keep workflow/policy changes separate from product changes.
- Every layer must leave a usable contract. Placeholder scaffolding is not a
  layer.
- Create sub-issues only for work that can be scheduled, assigned, or reused
  independently. PR layers are delivery mechanics, not extra roadmap nodes.
- Non-code decisions may still close by issue comment. Code always lands through
  the PR workflow above.
- The checked-in `ask-matt`, `implement`, and `code-review` skills carry the
  repo's committed-diff review order. A skills update must preserve that order
  and refresh their `skills-lock.json` hashes.
