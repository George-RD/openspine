# Issue tracker: Linear

Linear is the planner and the only issue tracker for this repo (since 22 Oct
2026). Specs, implementation tickets, wayfinder maps and the roadmap live in the
**[OpenSpine project](https://linear.app/another-workspace-for-me/project/openspine-be92ec0d1626)**
(`P-ANO-23`) of team **ANO**. Issue identifiers look like `ANO-123`.

Use the Linear MCP tools (`save_issue`, `get_issue`, `list_issues`,
`save_comment`, `list_comments`, …). Read an issue's description **and**
comments before acting: dated owner comments override older text.

GitHub Issues in `George-RD/openspine` are closed history. Each imported Linear
issue links its closed GitHub original, and `#N` inside imported text means
GitHub issue/PR N in this repo. Historical `#N` links in code, specs and docs
stay valid; do not rewrite them. Fetch one with `gh issue view <N> --comments`
(fall back to `gh pr view <N> --comments`: issues and PRs share one number
space). Do not open new GitHub issues; if an outside contributor files one,
triage it into Linear and close it with a link.

## Conventions

- **Create an issue**: `save_issue` with `team: "ANO"`,
  `project: "P-ANO-23"`, a `milestone` (below), `labels` including
  `repo:openspine`, and `priority`. Every issue sits in exactly one milestone.
- **Read an issue**: `get_issue` (`id: "ANO-123"`, `includeRelations: true`)
  plus `list_comments` (`issueId: "ANO-123"`).
- **List issues**: `list_issues` with `project: "P-ANO-23"`, filtered by
  `state`, `label` or `parentId`; request `projectMilestone` in `fields` to
  group by milestone (there is no milestone filter).
- **Comment**: `save_comment` with `issueId`.
- **Apply / remove labels**: `save_issue` with `addLabels` / `removeLabels`.
- **Close**: comment with the resolution, then `save_issue` with
  `state: "Done"` (or `"Canceled"` for wontfix).
- **States**: Backlog → Todo → In Progress → In Review → Done; Canceled,
  Duplicate.

## Milestones (roadmap order)

The roadmap is the project's milestones, then priority, then blocking
relations. The public `site/` roadmap is a summary generated from
`capabilities/capability-map.json`; Linear is canonical for order.

1. **Owner-ready**: George installs, selects Lyra, runs, chats, upgrades and
   rolls back from the documented path.
2. **Distribution**: signed, version-pinned artifacts; install without a source
   checkout.
3. **Immune system designs**: provenance labels, capability-derived tool
   catalogs, disclosure gating (designs; gates Bell serving real customers).
4. **Progressive delegation (post-fit-review)**: fenced until the Bell fit
   review; label `fence:post-fit-review`.
5. **Exploration**: research off the delivery path.

## Labels

- **Triage** group (single-select): see [triage-labels.md](triage-labels.md).
- **Wayfinder** group: `wayfinder:map`, `wayfinder:research`,
  `wayfinder:prototype`, `wayfinder:grilling`, `wayfinder:task`.
- **Kind** group: `Build` (ships code with tests), `Spike`, `Decision`,
  `Verification`.
- `fence:post-fit-review`, `needs-george` (only George can unblock: credentials,
  devices, money; the comment states the exact ask), `repo:openspine`.

## Pull requests

PRs stay on GitHub ([delivery.md](delivery.md)). Reference the Linear issue in
the PR body: `Part of ANO-123` on lower stacked layers, `Fixes ANO-123` on the
cumulative top or sole PR. After merging, confirm the Linear issue is Done and
comment the merge commit; set it yourself if the integration did not.

**PRs as a request surface: no.** _(Set to `yes` if this repo treats external
PRs as feature requests; `/triage` reads this flag.)_ When `yes`, external PRs
(`gh pr list --state open --json number,title,body,labels,author,authorAssociation,comments`,
keeping `CONTRIBUTOR`, `FIRST_TIME_CONTRIBUTOR` or `NONE`) are triaged by filing
or updating a Linear issue that links the PR.

## When a skill says "publish to the issue tracker"

Create a Linear issue in the OpenSpine project as above.

## When a skill says "fetch the relevant ticket"

`get_issue` and `list_comments` for its `ANO-` identifier.

## Wayfinding operations

Used by `/wayfinder`. The **map** is one Linear issue; its **child** issues
are the tickets.

- **Map**: an issue labelled `wayfinder:map`, holding the Notes /
  Decisions-so-far / Fog body, in the milestone its destination serves.
- **Child ticket**: an issue with `parentId` set to the map, labelled
  `wayfinder:<type>` (`research`/`prototype`/`grilling`/`task`). Once claimed,
  the ticket is assigned to the driving dev.
- **Blocking**: native Linear relations, set with `save_issue`
  `blockedBy: ["ANO-…"]` (or `blocks`). A ticket is unblocked when every
  blocker is Done or Canceled. Create the tickets first, then wire edges in a
  second pass.
- **Frontier query**: `list_issues` with `parentId` = the map and an open
  state, drop any with an open blocker (inspect relations with `get_issue`) or
  an assignee; first in map order (priority, then creation) wins.
- **Claim**: `save_issue` with `assignee: "me"` and `state: "In Progress"`,
  the session's first write.
- **Resolve**: `save_comment` with the answer, `save_issue` with
  `state: "Done"`, then append a context pointer (gist + link) to the map's
  Decisions-so-far.
