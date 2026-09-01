# openspine

Rust workspace (`crates/`) for a self-hostable governed-agent runtime, plus
historical OpenSpec specs and archived changes (`openspec/`) and an Astro docs
site (`site/`).

**Process canon — read first: [`DIRECTION.md`](DIRECTION.md).** New work uses
the Pocock skills workflow (wayfinder → grilling/domain-modeling → to-spec →
to-tickets → implement, all under `.agents/skills/`). The retired OpenSpec
ceremony is unavailable: never use it for delivery or create a new
`openspec/changes/` entry. Domain vocabulary: [`CONTEXT.md`](CONTEXT.md) (five
promises, six users, decision test) and [`STORIES.md`](STORIES.md).

Crates are split by trust boundary, not by convenience: `openspine-schemas`
(shared types) → `openspine-authority` (composition) → `openspine-gate`
(mediation) → `openspine-kernel` (privileged) → `openspine-shell` (untrusted,
sandboxed). Dependencies point one way down that list.

## Gate

`scripts/check.sh` is the gate and mirrors `.github/workflows/ci.yml`. It
strict-validates the canonical historical OpenSpec specs; archived change
packages remain static history. Inner loop: `cargo test -p <crate>`.

## Gotchas

- **The kernel E2E test spawns the real `openspine-shell` binary.** `cargo test`
  builds shell *tests* but not the shell *binary*, so a bare `cargo test
  --workspace` fails with a confusing spawn error. Run `cargo build -p
  openspine-shell --bin openspine-shell` first (check.sh does this for you).
- **500-line cap on `crates/**/*.rs`** (`scripts/check-file-sizes.sh`). The
  escape hatch is a first non-blank line reading
  `// openspine:allow-large-module reason: <reason>`. Prefer the repo's other
  habit: move the test module into `<name>_tests.rs` and pull it in with
  `#[cfg(test)] #[path = "<name>_tests.rs"] mod tests;`.
- **`capabilities/capability-map.json` is generated into the public roadmap.**
  `scripts/capability-map.mjs` rewrites the block between
  `<!-- capability-map:start -->` / `<!-- capability-map:end -->` in
  `site/src/content/docs/roadmap.md`.
  Edit the JSON, never inside the markers.
- **Every `test: <name>` row in `docs/threat-claims.md` must name a real test**
  (`scripts/check-claims.sh`). Writing a security claim without landing its test
  fails the gate.
- **OpenSpec is historical spec validation only.** It is pinned to exact
  `1.6.0-beta.1`; `scripts/check.sh` uses the local binary solely to run
  `validate --all --strict` over `openspec/specs/`. Archived change packages
  are not revalidated. Do not run the change ceremony. Rust is pinned to
  1.97.1 in `rust-toolchain.toml`.
- **Requirement history lives in the canon.**
  `.raw/openspine-agentos-design-log.md` and
  `.raw/openspine-decision-log.md` record settled product decisions; the
  archived OpenSpec tree records their former specification shape. Current
  process comes from `DIRECTION.md` and the issue tracker, never from the
  historical OpenSpec sequence.

## Where to look

- Process rulings: `DIRECTION.md`. PR and stack mechanics:
  `docs/agents/delivery.md`.
- Operator-facing docs: `docs/` (terminal chat, Gmail setup, day-2 ops, threat claims).
- Product framing and the public site: `site/`.
- Whole-codebase structural view: the **codegraph** MCP server, registered in
  `.omp/mcp.json` (`omo-codegraph`), so omp sessions here get `codegraph_explore`
  / `codegraph_search` / `codegraph_callers` / `codegraph_node` as tools.
  `explore` gives a symbol's blast radius, callers, and covering tests in one
  shot — use it instead of reading source file-by-file. The index is a local
  `.codegraph/codegraph.db` (~34MB, gitignored). It is NOT built on demand: run
  `~/.omo/codegraph/bin/codegraph index` once (~5s; the binary is not on `PATH`),
  after which the MCP file-watcher auto-syncs on edits. Re-run `index` after a
  branch switch or large rebase if results look stale.

## Agent skills

Portable engineering skills and their support files are versioned under
`.agents/skills/`; `skills-lock.json` records their sources. OMP-only extras
under `.omp/skills/` are supplemental and do not define delivery.

### Issue tracker

Issues live in GitHub Issues (`George-RD/openspine`), managed with the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical triage labels, used verbatim. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Delivery

Code lands on `main` through pull requests. One issue may use a short stack of
complete, tested, genuinely dependent layers. Review every layer against
standards and spec, then independently review the cumulative stack head. See
`docs/agents/delivery.md`.
