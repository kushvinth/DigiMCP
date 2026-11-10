<!-- mcp-name: io.github.kushvinth/quad -->
<div align="center">

# quad

### What's due, attendance, bunk maths, grades & agentic submissions.

**A lightning-fast MCP server and student intelligence CLI for Digiicampus / CollPoll. One binary, one token, 14 tools — and it plugs into every agent you run.**

[![CI](https://github.com/kushvinth/quad/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/kushvinth/quad/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/badge/crates.io-quad--mcp-f46623?style=flat-square&logo=rust&logoColor=white)](https://crates.io/crates/quad-mcp)
[![MCP server](https://img.shields.io/badge/MCP-14_tools-5c4ee5?style=flat-square)](#the-14-tools)
[![skills.sh](https://img.shields.io/badge/skills.sh-quad-111111?style=flat-square)](https://skills.sh/kushvinth/quad/quad)
[![MIT license](https://img.shields.io/badge/license-MIT-f4c430?style=flat-square)](LICENSE)

[Install & forget](#install--forget) · [The 14 tools](#the-14-tools) · [Built-in RTK](#built-in-rtk-token-compaction-60-80-savings) · [Works in your agent](#works-in-your-agent) · [Dashboard](#interactive-cli--dashboard) · [Privacy](#privacy)

```bash
quad login && quad setup
```

</div>

---

Every student portal answers the same three questions badly: *what's due*,
*how much can I still bunk*, and *what did I get*. You click through nested menus,
fight slow pagination, and miss deadlines buried in course tabs.

**quad** connects your agent directly to the portal's internal API. Zero browser
overhead, cold startup under 6 ms, exact IANA timezone timestamps, in-tree RTK
token compaction to keep agent context lean, and 14 native tools for both reading
records and submitting coursework.

---

## Install & forget

**macOS, and Linux with Homebrew:**
```bash
brew install kushvinth/tap/quad
```
*(or via cargo)*
```bash
cargo install quad-mcp
```

**Windows, with winget:**
```powershell
winget install kushvinth.quad
```

**Linux, macOS or WSL, with the install script:**
```bash
curl -fsSL https://raw.githubusercontent.com/kushvinth/quad/main/scripts/install.sh | sh
```

### Then plug it into your agent

```bash
# 1. Connect to your college portal (auto-detects session token from your browser)
quad login --portal <yourcollege>.digiicampus.com

# 2. Wire into Claude Code, Codex, Cursor, Gemini CLI, OpenCode
quad setup
```

Restart your agent. That's it: when it asks *"what's due this week?"*, quad answers in
6 ms with timezone-exact deadlines, attendance with bunk predictions, grades, or
submits feedback forms for you. Nothing to learn, nothing to call, and upgrading quad
never changes this.

> **Note on crates.io**: The package name on crates.io is **`quad-mcp`** (which compiles
> the `quad` binary). Check you have this one: `quad doctor` must show the diagnostic
> health checks, not "command not found".

[Pre-built binaries & release assets](https://github.com/kushvinth/quad/releases) · [Per-agent manual setup](#works-in-your-agent) · [Dry-run previews](#agentic-actions--dry-run-preview)

---

## Built-in RTK token compaction (60–80% savings)

Student portals return enormous JSON payloads packed with raw HTML descriptions,
redundant navigation fields, inline base64 images, and empty keys. Quad has an
in-tree **RTK-style token compactor** (`src/simplify.rs`) that runs directly in
memory before returning results to your agent:

- **HTML stripping**: Compresses verbose rich-text descriptions down to concise text.
- **URI neutralization**: Replaces multi-kilobyte data URIs with compact placeholders.
- **Whitespace normalization**: Eliminates wasted runs of indentation and spaces.
- **Dead-field pruning**: Drops nulls and uninformative keys while preserving lists.

Your agent gets 100% of the relevant data while saving 60% to 80% of context window
tokens on every single tool call — with no external dependencies or daemons.

---

## The 14 tools

You almost never need to look up an ID. Pass human names like `class: "Physics"` or
`assignment: "oscillations"` and quad resolves them automatically.

### Intelligence & Academics (Read)

| Tool | What it does |
|---|---|
| `due` | Merged list of upcoming assignments, quizzes, and exams sorted by urgency. Flags overdue work and includes `hoursUntilDue`. |
| `attendance` | Overall and per-course percentages, plus **`bunksLeft`** (classes you can skip while staying $\ge 75\%$) and **`bunksNeeded`** (classes needed to recover). |
| `grades` | Published results and component marks per subject. Unpublished items report `published: false` rather than guessing. |
| `assignment` | Deep dive on one assignment: description, due date, submission status, grades, remarks, and fresh signed download links. |
| `assignments` | Class-by-class course view: upcoming, previous, and draft items. |
| `classes` | All enrolled courses, sections, and component IDs (lectures, labs, tutorials). |
| `search` | Full-text search across all assignment titles and descriptions. |
| `account` | Session status: student name, roll number, college portal origin, and days until cookie expiry. |
| `feedback_forms` | Active faculty and course evaluation questionnaires with submission status. |
| `quiz_questions` | Questions and options for an active quiz so you can inspect them before answering. |

### Agentic Actions & Submissions (Write)

Action tools perform live portal operations by default and include full support for
`dry_run: true` so you can validate inputs and compliance before submitting.

| Tool | What it does |
|---|---|
| `submit_assignment` | Submit an assignment with answer text and optional attachment links. |
| `submit_quiz` | Submit answers for an active portal quiz. |
| `fill_feedback` | Complete faculty/course feedback forms with automated rating distributions and constructive remarks. |
| `fill_form` | Generic campus survey and form submitter. |

#### Agentic actions & dry-run preview

To preview any submission without sending it to the portal, simply set `dry_run: true`:

```json
{
  "form": "CS3803",
  "dry_run": true
}
```

Quad returns the validated payload, course faculty ID, resolved template ID, and
the exact payload shape without mutating live portal state.

---

## Works in your agent

`quad setup` inspects your machine and safely configures these agents automatically.
If you prefer manual configuration, use the snippets below:

### Claude Code

```bash
# Register tool via CLI
claude mcp add --scope user quad -- quad

# Or install the official plugin (skills + MCP server in one command)
claude plugin marketplace add kushvinth/quad
claude plugin install quad@quad
```

### Codex

Add to `~/.codex/config.toml`:

```toml
[mcp_servers.quad]
command = "quad"
args = []
```

### Cursor

Add to `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "quad": {
      "command": "quad",
      "args": []
    }
  }
}
```

### Gemini CLI

Add to `~/.gemini/settings.json`:

```json
{
  "mcpServers": {
    "quad": {
      "command": "quad",
      "args": []
    }
  }
}
```

### OpenCode

Add to `~/.config/opencode/opencode.json`:

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "quad": {
      "type": "local",
      "command": ["quad"],
      "enabled": true
    }
  }
}
```

### Any other MCP host

Quad is a standard MCP stdio server with no extra arguments required:

```json
{
  "command": "quad",
  "args": []
}
```

---

## Interactive CLI & Dashboard

Besides serving MCP over stdio, `quad` is a full student terminal utility:

### Login & auto-detection

```bash
# Interactive browser sign-in (auto-detects session token from Zen, Chrome, Arc, Brave, Firefox, Edge, etc.)
quad login --portal snuc.digiicampus.com

# Direct token login
quad login --token eyJhbGci...

# Connect via Chrome DevTools Protocol
quad login --connect
```

### System doctor

```bash
quad doctor
```
Verifies token validity, portal connectivity, server clock drift, and active course enrollments.

### Terminal dashboard

```bash
quad dashboard
```
Renders a two-column student dashboard featuring DigiiCampus gradient FIGlet banners,
attendance meters, bunk calculations, and upcoming deadlines.

---

## Configuration & environment

Tokens are saved locally with strict filesystem permissions (`chmod 600`) at
`~/.config/quad/config.toml`. You can also configure quad via environment variables or a
`.env` file in your workspace:

| Variable | Purpose | Default |
|---|---|---|
| `QUAD_TOKEN` | Portal session token (`user` cookie starting with `eyJ`) | From `config.toml` |
| `QUAD_BASE_URL` | Portal origin (e.g. `https://snuc.digiicampus.com`) | Derived from JWT |
| `QUAD_TIMEZONE` | College IANA timezone | `Asia/Kolkata` |
| `QUAD_TIMEOUT_MS` | Per-request network timeout | `40000` |
| `QUAD_CONFIG_DIR` | Custom directory for config file storage | `~/.config/quad` |

---

## Privacy & safety

- **Zero telemetry**: No analytics, tracking, or telemetry pings. Network requests only go to your college's portal.
- **Token stays local**: Tokens are stored with `chmod 600` permissions and sent solely in the `Auth-Token` header to your portal origin.
- **Accurate timezones**: Deadlines are mapped to your institution's IANA timezone and formatted in RFC 3339, eliminating cross-timezone deadline confusion.
- **Controlled write operations**: Write operations support `dry_run: true` preview mode.

---

## FAQ

<details><summary>Which portals does quad support?</summary>

Any institution running on Digiicampus or CollPoll (e.g. `<institute>.digiicampus.com`). It targets student accounts and handles authentication cookies, dynamic session parameters, and evaluation templates.
</details>

<details><summary>Why Rust?</summary>

Quad starts cold in under 6 milliseconds, requires ~6 MB of disk space, and has zero external runtime requirements (no Node.js, no Python virtualenvs). It stays idle with zero memory overhead when your agent isn't calling it.
</details>

<details><summary>How does the browser auto-detection work?</summary>

`quad login` opens your default browser to your college portal. Once you authenticate, quad scans local browser cookie stores (Chromium-based browsers, Firefox, Zen, Arc, Safari) and Snappy-compressed LevelDB logs to automatically extract the session token without manual copy-pasting.
</details>

---

## Development

```bash
# Run unit & e2e test suite (against mock portal)
cargo test --all-targets

# Linter & formatter
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check

# Build release binary
cargo build --release
```

---

## License

[MIT](LICENSE). Not affiliated with Digiicampus or CollPoll.
