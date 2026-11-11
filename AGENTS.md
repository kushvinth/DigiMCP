# DigiMCP

Guidance for agents working **on** this repository (not for agents using the
MCP server — that lives in `skills/digimcp/SKILL.md`).

## What this is

A high-performance Rust MCP server and student CLI, **`DigiMCP`**, that provides:

1. **`digi` (no args)** — serves MCP over stdio. 16 tools live in `src/registry.rs`
   (declarations) and `src/tools.rs` (behaviour).
2. **`digi login` / `setup` / `doctor` / `dashboard`** — onboarding, agent wiring,
   and clean two-column terminal dashboard.
3. Binary aliases: `digi` (primary CLI), `digii`, and `digimcp`.

Read operations are pure GETs. Interactive and write operations (`submit_assignment`,
`submit_quiz`, `fill_feedback`, `fill_form`) perform portal write actions directly,
and support `dry_run: true` preview validation. All write operations are journaled
and guarded against accidental duplicate submissions via `~/.local/share/digimcp/journal.jsonl`.

## Layout

```
src/lib.rs       DigiMCP library root
src/cli.rs       CLI dispatch + MCP serve loop
src/main.rs      digi binary entrypoint
src/bin/digimcp.rs digimcp binary entrypoint
src/server.rs    Quad state, ServerHandler impl, ok/fail helpers
src/registry.rs  explicit tool registry (16 tools, snake_case + camelCase alias schemas)
src/tools.rs     one function per question; pure helpers + class resolution
src/portal.rs    HTTP client, route wrappers, timezone-correct date parsing
src/config.rs    token/origin resolution (DIGIMCP_* env → .env → config.toml), save/load
src/journal.rs   append-only write journal and idempotency protection
src/login.rs     digi login (paste token, or read it from Chrome devtools)
src/setup.rs     digi setup (agent detection, safe config edits, --remove)
src/simplify.rs  in-tree RTK token killer & output shaping
src/ui.rs        two-column terminal dashboard, spinners, DigiiCampus gradient FIGlet
skills/digimcp/  the agent-facing SKILL.md, embedded into the binary
tests/e2e.rs     stub portal + real binary over MCP (10 end-to-end tests)
Formula/digimcp.rb Homebrew formula
```

## Rules that keep it correct

- **Tool wiring is explicit.** Do not switch to `#[tool_router]`: a bad
  attribute inside it silently drops every tool from `tools/list`. The registry
  test `registers_every_tool` is the tripwire.
- **Never render a naive portal timestamp directly.** Use `portal::rfc3339` /
  `hours_until`; they apply the college's IANA zone. A naive parse shifts every
  deadline for anyone whose laptop is not in IST.
- **Errors are prose for the agent.** Return `DigiError` variants; they render as
  "Session expired — refresh the token (…)". Never leak the token or a full URL
  with query strings into a message. Secrets are redacted automatically.
- **Resolve names, not ids.** Tools take a class or assignment *name*; use
  `pick_class` and the name matcher in `tools.rs`.
- **Zero fiction on the write path.** Never invent praise or remarks for unanswered
  questions, and never default missing ratings to 5.0. Mandatory questions must be
  validated against live template before submission or dry-run approval.
- **Audit every write.** Record write actions in `~/.local/share/digimcp/journal.jsonl`
  to guarantee auditability and prevent duplicate submissions.
- **Config edits must be reversible and guarded.** `setup` prints before it
  writes, backs up, re-parses what it wrote, and refuses files it cannot parse.

## Dev loop

```bash
cargo test                       # 17 unit + 10 e2e tests (fake portal, no account needed)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
./scripts/validate-manifests.sh  # verifies version sync across 6 manifests
cargo build --release
```

The e2e suite is the contract: it boots a stub of the private routes and drives
the real binary over MCP. Add a case there before changing a tool's output
shape.

## Versioning

Version lives in `Cargo.toml` and is mirrored in `.claude-plugin/plugin.json`,
`.claude-plugin/marketplace.json`, `.codex-plugin/plugin.json`,
`gemini-extension.json`, `server.json`, `plugin.yaml`, and `Formula/digimcp.rb`.
Verify consistency with `./scripts/validate-manifests.sh`.
