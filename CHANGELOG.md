# Changelog

## 0.1.0

First release. Replaces the TypeScript `fijii-mcp` bootstrap.

- **Rust rewrite.** One static binary, no Node runtime, no vendored SDK
  submodule, no `node_modules`. Boots in milliseconds.
- **Eight tools instead of fourteen**, one per student question: `due`,
  `attendance`, `grades`, `assignment`, `assignments`, `classes`, `account`,
  `search`. Quizzes and exams are merged into `due`.
- **Names, not ids.** `class: "Physics"` and `assignment: "oscillations"` are
  resolved by id, course code, name or token overlap.
- **Deadlines are real instants.** Naive portal timestamps are converted with
  the college's IANA zone and returned as RFC 3339, with `hoursUntilDue`.
- **Attendance that answers the real question:** `bunksLeft` and `bunksNeeded`
  against the 75% minimum.
- **Zero-config onboarding.** The token carries its own portal origin, so
  `quad login` is the only setup step; `quad doctor` verifies each link in the
  chain separately.
- **`quad setup`** detects Claude Code, Codex, Cursor, Gemini CLI and opencode,
  shows the changes, backs up every file, refuses configs it cannot parse, and
  supports `--remove`.
- **One-command installs** for Claude Code (plugin), crates.io, and any MCP host
  via `"command": "quad"`.
- **Cross-platform CI** with clippy denied warnings, rustfmt, unit tests and an
  end-to-end suite against a stub portal.
