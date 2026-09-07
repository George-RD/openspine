# Inspect a local assistant package

Inspect package structure and exact content without starting OpenSpine:

```sh
openspine package inspect artifacts/lyra
openspine package inspect /path/to/candidate --json
```

Inspection needs no configuration, owner keys, account connections, model or
running kernel. It does not load an adjacent `openspine.env`, create application
state, change `lyra_dir`, install a package or select one for use. Linux and macOS
provide the descriptor-relative source capture used by this command.

## What a successful result means

The schema-v1 declaration matches the captured artifacts in all seven supported
families: agents, routes, workflows, packs, templates, policies and golden sets.
The existing base loader validates their types, preserves highest-version
selection and rejects duplicate identities. The entry agent must resolve to an
active agent. Golden sets retain their native, unversioned identity.

Success means **structurally validated**, not compatible with every deployment,
publisher-verified, approved or safe to activate. Explicit local paths are always
reported as `local-unverified`, even when the package calls itself `lyra` or the
path points to the bundled source. Descriptions, persona documents and security
statements do not grant authority.

The command captures bytes once. Validation uses a private temporary copy of
those captured bytes, not another read of the candidate. The temporary copy is
removed afterward. Source changes after capture cannot replace the validated
snapshot handed to a later installer.

## Accepted layout and limits

The root contains `package.yaml`, the supported artifact directories and ordinary
`.md`/`.txt` documentation. A flat `docs/` directory is also accepted. Artifact
files must use `.yaml` or `.yml`. Nested directories, unknown artifact families,
base personas, hooks and other payloads are rejected rather than ignored.

Files must be non-executable UTF-8 text without NUL bytes. Symbolic links, hard
links and special files are rejected. Relative path components use ASCII letters,
digits, underscores, hyphens or periods, up to 128 bytes; trailing periods,
Windows device names and case-ambiguous file paths are rejected. These intake
restrictions do not change the existing live loader's rules.

Limits are enforced during enumeration and reading: 4,096 files, 8 MiB per file
and 64 MiB total. Read/enumeration errors fail inspection. Diagnostics do not echo
candidate contents or unchecked paths; text output is a bounded summary.

## JSON and identity

`--json` writes a single object to stdout. Successful reports have
`schema_version: 1`, `inventory_format_version: 1`, `valid: true`, `provenance`,
`package_id`, positive integer `revision`, `content_digest` and `inventory`.
Each inventory entry has `path`, `bytes` and `digest`; entries are sorted by
normalized relative path. Declaration and documentation bytes participate.
Free-form metadata and artifact bodies are not included in the report.

Each file digest is the existing `sha256:<lowercase hex>` digest of its raw bytes.
The content digest uses OpenSpine's canonical JSON digest helper on:

```json
{"inventory_format_version":1,"files":[{"path":"...","bytes":123,"digest":"sha256:..."}]}
```

Absolute source paths, timestamps, permissions and directory enumeration order
are not part of this identity. A file-byte or relative-path change is.

Validation failures exit nonzero and produce `schema_version: 1`, `valid: false`,
`provenance: "local-unverified"` and an `error` with a stable `code` and bounded
`message`. Invalid command-line syntax remains a normal CLI usage error.

## Not shipped by inspection

Inactive installation and audit receipts are tracked separately in #275. Package
selection/activation, remote fetch and publisher signature verification are not
implemented by this command. The existing `lyra_dir` runtime path remains
unchanged. Inspection alone does not complete installation epic #117.
