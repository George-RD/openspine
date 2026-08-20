# openspine

Rust workspace (`crates/`) for a self-hostable governed-agent runtime, plus an
OpenSpec spec tree (`openspec/`) and an Astro docs site (`site/`).

Crates are split by trust boundary, not by convenience: `openspine-schemas`
(shared types) → `openspine-authority` (composition) → `openspine-gate`
(mediation) → `openspine-kernel` (privileged) → `openspine-shell` (untrusted,
sandboxed). Dependencies point one way down that list.

## Gate

`scripts/check.sh` is the gate and mirrors `.github/workflows/ci.yml`. Pass a
change id (`scripts/check.sh <change-id>`) to strict-validate one in-flight
OpenSpec change instead of all of them. Inner loop: `cargo test -p <crate>`.

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
- **Archive changes with the CLI, never by moving directories.**
  `npx --no-install openspec archive "<name>" --yes` applies the deltas into
  `openspec/specs/` mechanically; a raw `mv` into `openspec/changes/archive/`
  leaves the specs unapplied. `--yes` is permitted on `archive` only. If it
  fails with `ADDED ... already exists`, the change re-`ADDED` a pre-seeded
  requirement — change the delta header to `## MODIFIED Requirements` and
  re-run; do not fall back to `--skip-specs`.
- **`.omp/skills/openspec-*` and `.omp/commands/opsx-*` are generated but
  hand-patched** with that ceremony. `openspec init/update --tools oh-my-pi`
  silently reverts the patch; `scripts/check-omp-ceremony.sh` catches it.
  Re-apply before committing regenerated output. `.omp/` is the only
  maintained copy — do not re-generate openspec skills into other harness
  directories, they drift and teach the raw-move archive.
- **OpenSpec is pinned to exact `1.6.0-beta.1`**; use `node_modules/.bin/openspec`
  (or `$OPENSPINE_OPENSPEC_BIN`), not a global install. Rust is pinned to 1.97.1
  in `rust-toolchain.toml`.
- **Requirement content lives in the canon, not in the sequence file.**
  `.raw/openspine-agentos-design-log.md` (AD-0XX, only *settled* entries bind a
  spec) and `.raw/openspine-decision-log.md` (D-0XX) are canon;
  `openspec/openspine-change-sequence.md` holds only decomposition and
  dependency edges. On conflict, canon wins.

## Where to look

- OpenSpec workflow detail (propose / apply / archive): `.omp/skills/openspec-*/SKILL.md`.
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
