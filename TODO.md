# DigiMCP — engineering backlog

Product-level weaknesses found by walking the real portal end to end (Sem 5,
9 class sections, real feedback forms) and reading `src/`. Ordered by blast
radius, not effort.

Legend: **P0** = writes wrong data / lies to the agent · **P1** = product gap a
student hits immediately · **P2** = architecture that will not survive scale ·
**P3** = hygiene.

---

## P0 — Correctness: the write path can submit fiction under the student's name

- [x] **`dry_run` validates live template coverage.** In `fill_feedback`, `dry_run`
      fetches the dynamic questionnaire template, diffs mandatory questions,
      refuses empty `ratings: []`, and validates 100% required coverage.
- [x] **Zero ratings silently becomes all-Excellent.** Removed `unwrap_or(5.0)`.
      Mandatory questions require explicit scores; unrated questions are never
      defaulted to perfect scores.
- [x] **Subjective questions default to fabricated praise.** Removed fake praise
      string `"Insightful course material and very well-structured instruction."`.
      Subjective questions now require explicit answers or empty strings if optional.
- [x] **`fill_feedback` picks the first substring match and ignores status.**
      Filters out `SUBMITTED` forms unless `resubmit: true` is passed, prefers
      exact matches, and refuses ambiguous matches with full conflict diagnostics.
- [x] **Idempotency and write journal.** Implemented append-only write journal
      in `src/journal.rs` saving to `~/.local/share/digimcp/journal.jsonl`.
      Refuses accidental duplicate submissions across `submit_assignment`,
      `submit_quiz`, `fill_feedback`, and `fill_form` without `force: true` or `resubmit: true`.
- [ ] **No confirmation gate on destructive writes.** Keeping the action tools
      is fine, but there is no `confirm: true` for irreversible calls, so one
      misread prompt submits. Feedback and quiz submissions cannot be edited
      afterwards — this matters more than for assignments.
- [x] **`attendance` reports 100% for a course with zero marked classes.**
      Courses with total classes = 0 return `percentage: null`, `status: "no-data"`,
      and `bunksLeft: null`.
- [x] **`bunksLeft` unmarked lessons model.** Tracks `unmarkedLessons`, computes
      worst-case percentages if unmarked classes become absences, and surfaces
      high-uncertainty warnings when unmarked lessons exceed 20%.

## P1 — Product gaps a student hits on day one

- [x] **Course-material tools (`materials` and `material`).** Implemented
      module-grouped course notes, slides, problem sets, and temporary fresh
      signed download links (`src/tools.rs`).
- [ ] **No timetable, no exam schedule, no announcements.** "When is my exam"
      and "what did the department announce" are half the portal. The SPA calls
      `/api/timetable/*` and post/announcement routes; none are wired.
- [x] **Argument naming contradictions resolved.** Registry schemas use `snake_case`
      (`question_id`, `class_id`) and include `#[serde(alias = "...")]` to accept
      `camelCase` (`questionId`, `classId`) seamlessly.
- [x] **Stale tool descriptions refreshed.** Registry and `SKILL.md` documents
      all 16 tools, required coverage, and dry-run preview guarantees.
- [ ] **`digi setup` verification.** Add `--verify` flag that runs the configured
      command in each agent and reports the MCP handshake.

## P2 — Architecture

- [ ] **Two API generations with a silent, shape-changing fallback.**
      Detect the API generation once at `account`, cache it, and report which is in use.
- [ ] **Silent degradation hides real failures.** Collect fallback reasons into the response.
- [x] **Client reuse across concurrent tasks.** Replaced `Portal::new` inside `JoinSet`
      with `p.clone()`, sharing TLS session cache and HTTP connection pools across concurrent queries.
- [ ] **No pagination anywhere.** `grades`, `search`, `due` and `feedback_forms`
      assume the portal returns everything.
- [ ] **No observability.** Add `tracing` and `--verbose` for detailed execution logs.
- [x] **Secret leak prevention in errors.** `redact_secrets` strips JWT tokens,
      query parameters (`?Signature=`, `Auth-Token`, etc.) before errors surface to agents.
- [ ] **Token is plaintext with 0600.** Support macOS Keychain and `op run` credential storage.
- [ ] **Multi-profile and institution policy.** Add `--profile` support for multiple institutions.
- [x] **Version synchronization across ecosystem manifests.** Automated verification
      across `Cargo.toml`, `.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json`,
      `.codex-plugin/plugin.json`, `gemini-extension.json`, `server.json`, `plugin.yaml`,
      and `Formula/digimcp.rb` via `./scripts/validate-manifests.sh`.
- [ ] **Split `login.rs` into submodules.** Split browser scanning, token decoding, and CDP.
- [ ] **Measure RTK savings with benchmark fixtures.** Add automated bytes-in vs bytes-out tests.

## P3 — Hygiene

- [x] **E2E tests for error paths.** Added automated tests in `tests/e2e.rs` for:
      - `feedback_empty_ratings_fails_validation`
      - `feedback_ambiguous_and_submitted_checks`
      - `attendance_zero_total_lessons_reports_no_data`
- [ ] **The stub portal encodes our assumptions, not the portal's behaviour.**
      Capture redacted fixtures from the live API and replay them.
- [ ] **JSON-RPC error frames are invisible to the CLI.**
- [x] **`--json` mode for `doctor` and `dashboard`.** Added `--json` flag to both commands
      for structured machine output.

---

## Done

- [x] Rebranded entire crate, CLI, manifests, and documentation from `quad` to `DigiMCP`.
- [x] Primary CLI command is `digi` with `digii` and `digimcp` aliases.
- [x] Replaced multiline wrapped tools text dump in dashboard with clean Two-Column Session & Capabilities view.
- [x] Implemented 16 MCP tools (including `materials` and `material`).
- [x] Added append-only write journal (`~/.local/share/digimcp/journal.jsonl`).
- [x] Re-rendered visual assets with `kushvint@macaroon`, studio backdrop, and 60fps/20fps high quality.
- [x] Passing all 17 unit tests, 10 e2e tests, clean clippy, and manifest validation.
