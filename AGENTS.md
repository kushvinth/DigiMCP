# quad

Guidance for agents working **on** this repository (not for agents using the
MCP server — that lives in `skills/quad/SKILL.md`).

## What this is

A single Rust binary, `quad`, that does two jobs:

1. **`quad` (no args)** — serves MCP over stdio. Tools live in `src/registry.rs`
   (declarations) and `src/tools.rs` (behaviour).
2. **`quad login` / `setup` / `doctor` / `dashboard`** — onboarding, agent wiring,
   and Hermes-style terminal dashboard.

Read operations are pure GETs. Interactive and write operations (`submit_assignment`,
`submit_quiz`, `fill_feedback`, `fill_form`) require explicit execution and support
`dry_run: true` compliance validation before dispatching to the live portal.

## Layout

```
src/main.rs      CLI dispatch + MCP serve loop
src/server.rs    Quad state, ServerHandler impl, ok/fail helpers
src/registry.rs  explicit tool registry (name, schema, handler glue)
src/tools.rs     one function per question; pure helpers + class resolution
src/portal.rs    HTTP client, route wrappers, timezone-correct date parsing
src/config.rs    token/origin resolution (env → .env → config.toml), save/load
src/login.rs     quad login (paste token, or read it from Chrome devtools)
src/setup.rs     quad setup (agent detection, safe config edits, --remove)
src/simplify.rs  in-tree RTK token killer & output shaping
src/ui.rs        two-column terminal dashboard, spinners, DigiiCampus gradient FIGlet
skills/quad/     the agent-facing SKILL.md, embedded into the binary
tests/e2e.rs     stub portal + real binary over MCP
```

## Rules that keep it correct

- **Tool wiring is explicit.** Do not switch to `#[tool_router]`: a bad
  attribute inside it silently drops every tool from `tools/list`. The registry
  test `registers_every_tool` is the tripwire.
- **Never render a naive portal timestamp directly.** Use `portal::rfc3339` /
  `hours_until`; they apply the college's IANA zone. A naive parse shifts every
  deadline for anyone whose laptop is not in IST.
- **Errors are prose for the agent.** Return `QuadError` variants; they render as
  "Session expired — refresh the token (…)". Never leak the token or a full URL
  with query strings into a message.
- **Resolve names, not ids.** Tools take a class or assignment *name*; use
  `pick_class` and the name matcher in `tools.rs`.
- **Config edits must be reversible and guarded.** `setup` prints before it
  writes, backs up, re-parses what it wrote, and refuses files it cannot parse.

## Dev loop

```bash
cargo test                       # unit + e2e (fake portal, no account needed)
cargo clippy --all-targets
cargo fmt
cargo build --release
```

The e2e suite is the contract: it boots a stub of the private routes and drives
the real binary over MCP. Add a case there before changing a tool's output
shape.

## Versioning

Version lives in `Cargo.toml` and is mirrored in `.claude-plugin/plugin.json`,
`.codex-plugin/plugin.json`, `gemini-extension.json`, `server.json` and
`plugin.yaml`. Bump all six together (see `CHANGELOG.md`).
