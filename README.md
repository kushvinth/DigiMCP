<!-- mcp-name: io.github.kushvinth/quad -->
<div align="center">

# quad

### What's due, what's my attendance, what did I get.

**A read-only MCP server for your college portal, written in Rust. One binary, one token, eight tools — and it speaks to every agent you already run.**

[![CI](https://github.com/kushvinth/quad/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/kushvinth/quad/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/badge/crates.io-quad--mcp-f46623?style=flat-square&logo=rust&logoColor=white)](https://crates.io/crates/quad-mcp)
[![MCP server](https://img.shields.io/badge/MCP-8_tools-5c4ee5?style=flat-square)](#the-eight-tools)
[![skills.sh](https://img.shields.io/badge/skills.sh-quad-111111?style=flat-square)](https://skills.sh/kushvinth/quad/quad)
[![MIT license](https://img.shields.io/badge/license-MIT-f4c430?style=flat-square)](LICENSE)

[Quick start](#quick-start) · [The eight tools](#the-eight-tools) · [Agents](#wire-up-your-agents) · [Privacy](#privacy) · [FAQ](#faq)

```bash
quad login && quad setup
```

</div>

---

Every student portal answers the same three questions badly: *what's due*,
*how much can I still bunk*, *what did I get*. You end up clicking through five
screens to find out, and you still miss the deadline that was buried under a
nested module. quad answers them in one tool call, from your own session, with
no scraping and no browser open.

It is a single static Rust binary that speaks MCP over stdio. Your agent asks;
quad resolves the ids, converts the portal's naive timestamps into real
instants in your college's timezone, and hands back compact JSON.

> **Read-only by construction.** Every route quad calls is a GET. It cannot
> submit an assignment, start a quiz or post to a forum — the tool schemas say
> `readOnlyHint: true`, and there is no write path in the code at all.

## Quick start

```bash
# 1. Get it (any of these)
cargo install quad-mcp          # from crates.io
npx quad-mcp setup              # or run without installing (downloads the binary)

# 2. Connect it to your portal — it finds the token for you
quad login

# 3. Wire it into every agent on this machine
quad setup
```

`quad setup` finds Claude Code, Codex, Cursor, Gemini CLI and opencode, prints
exactly which files it will change, and applies them only after you say yes. It
registers the MCP server, installs the skill (so the agent knows *when* to reach
for quad), backs up every file it edits, and never rewrites a config it cannot
parse — it prints the snippet to paste instead. `quad setup --remove` undoes it
the same way.

Then just ask:

> "what's due this week?" · "am I below attendance in Physics?" · "how did the
> last lab go?" · "find the assignment that mentioned the SHM write-up"

## The eight tools

| Tool | The question it answers |
|---|---|
| `due` | Everything still due, **assignments and quizzes merged**, sorted by deadline, overdue flagged. `within_hours: 168` for the week. |
| `attendance` | Percentage per course and component, plus **`bunksLeft`** — how many classes you can still miss at 75% — and `bunksNeeded` to climb back. |
| `grades` | Published results and component marks. Unpublished results say `published: false` instead of inventing a number. |
| `assignment` | One item by **name**: description, attachments with fresh signed links, submission state, marks, remarks. |
| `assignments` | The class-by-class view: upcoming, previous, drafts, exams. |
| `classes` | What you're enrolled in, with the ids other tools use. |
| `account` | Who you're signed in as, which portal, which timezone — and whether the token still works. |
| `search` | Full-text search over assignment titles and descriptions. |

You almost never pass an id. Pass `class: "Physics"` or
`assignment: "oscillations"` and quad resolves it — by id, course code, name, or
all words present.

**Times are real instants.** The portal stores deadlines as naive strings like
`2026-11-09 23:59:00` in college-local time. Parsed naively, every deadline
shifts by hours on a laptop set to another zone. quad converts them with the
college's IANA zone and returns RFC 3339 (`2026-11-09T23:59:00+05:30`), plus
`hoursUntilDue` so the agent can reason about urgency without doing date maths.

## Wire up your agents

`quad setup` writes these for you; here they are if you'd rather do it by hand.

**Claude Code** — one command, plus the skill:
```bash
claude mcp add --scope user quad -- quad
```

**Claude Code plugin** — skill *and* server in one install:
```bash
claude plugin marketplace add kushvinth/quad
claude plugin install quad@quad
```

**opencode** (`~/.config/opencode/opencode.json`):
```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "quad": { "type": "local", "command": ["quad"], "enabled": true }
  }
}
```

**Codex** (`~/.codex/config.toml`):
```toml
[mcp_servers.quad]
command = "quad"
args = []
```

**Cursor** (`~/.cursor/mcp.json`) and **Gemini CLI** (`~/.gemini/settings.json`):
```json
{ "mcpServers": { "quad": { "command": "quad", "args": [] } } }
```

**Any other MCP host** — it's a stdio server with no arguments:
```json
{ "command": "quad", "args": [] }
```

The repo ships `.claude-plugin/`, `.codex-plugin/`, `.mcp.json`,
`gemini-extension.json` and `skills/quad/SKILL.md`, so most hosts pick it up
without configuration.

## Logging in

The portal treats the `user` cookie as the session token. There are three ways
to get it into quad, and only the first one is your problem:

```bash
quad login --portal mycollege.digiicampus.com     # prompts, validates, saves (chmod 600)
quad login --connect                              # reads it from Chrome on :9222
quad login --token eyJhbGciOi...                  # or paste it directly
quad doctor                                       # verify the whole chain
```

A JWT contains its own `instituteUrl`, so **the token alone is enough** — quad
derives the portal origin from it. The token lives in `~/.config/quad/config.toml`
(mode 600) and `QUAD_TOKEN` overrides it for a single session.

Sessions expire every few weeks. `quad doctor` and the `account` tool both tell
you how long is left, and the agent is instructed to walk you back through
`quad login` instead of failing cryptically.

| Variable | Purpose |
|---|---|
| `QUAD_TOKEN` | Session token (overrides the saved config) |
| `QUAD_BASE_URL` | Portal origin, when the token has no `instituteUrl` claim |
| `QUAD_TIMEZONE` | College IANA timezone (default `Asia/Kolkata`) |
| `QUAD_TIMEOUT_MS` | Per-request timeout (default 40000) |
| `QUAD_CONFIG_DIR` | Where the token lives (default `~/.config/quad`) |

`.env` files in the working directory and `~/.config/quad/` are read too, so you
can keep a per-project token without touching your shell.

## Privacy

Your transcript is not a public artifact, so quad treats it accordingly:

- **The token never leaves your machine** except as the `Auth-Token` header to
  your own portal origin. quad never prints it, never logs it, and never sends
  it anywhere else.
- **The token file is 600.** `quad login` sets that itself.
- **No telemetry, no analytics, no crash reporting.** The only network calls are
  to the portal you configured.
- **Personal fields stay off by default.** quad doesn't fetch email, phone or
  photos; the tools return coursework, attendance and marks only.
- **Nothing is written to the portal.** Read-only, by construction.

## FAQ

<details><summary>Which portals does this work with?</summary>

Digiicampus / CollPoll student portals (the ones at `<institute>.digiicampus.com`).
It targets the *student* role: faculty-only routes answer with an empty-body 403
and quad reports that clearly rather than pretending the data is empty.
</details>

<details><summary>Why Rust?</summary>

The server answers the MCP handshake in ~6 ms on a cold start, so the agent pays no penalty for
having it available; it's one ~6 MB static binary with no Node, no Python, no
`node_modules` and no runtime to install; and a tool that talks to a private,
undocumented API needs a compiler that catches the API shape drifting, which
typed JSON handling and 20+ tests do. Boot cost matters because MCP servers are
spawned per session and sit idle 99% of the time.
</details>

<details><summary>How is this different from scraping the portal?</summary>

No browser, no HTML parsing, no fragile selectors, no login automation. It uses
the same JSON routes the portal's own frontend calls, with your session, and
normalises their quirks (non-breaking spaces, paginated buckets, naive
timestamps, `perPage` being mandatory on the forum route).
</details>

<details><summary>Can it submit my assignment or answer a quiz for me?</summary>

No. There is no write code path in the repository — not disabled, not gated,
absent. That's deliberate: an agent with `submit` is one hallucinated tool call
away from an academic integrity problem.
</details>

<details><summary>The portal changed something and a tool broke. What now?</summary>

Run `quad doctor`. It checks the token, the origin, your profile, the class list
and the term list separately, so the failing step is obvious. File an issue with
that output — it contains no personal data beyond the college name.
</details>

## Development

```bash
cargo test          # unit tests + an end-to-end run against a fake portal
cargo build --release
./target/release/quad setup --yes --remove   # clean up after experimenting
```

The e2e suite boots a stub of the private routes, points the real binary at it,
and drives it over MCP exactly as an agent would — so the timezone maths,
pagination, name resolution and error messages are all covered without a live
account.

## License

[MIT](LICENSE). Not affiliated with Digiicampus or CollPoll. Use only accounts
you're authorised to use, and respect your institution's terms.
