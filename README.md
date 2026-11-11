<!-- mcp-name: io.github.kushvinth/digimcp -->
<div align="center">

# DigiMCP (`digi`)

### What's due, attendance, bunk maths, course materials, grades & agentic submissions.

**A lightning-fast MCP server and student intelligence CLI for DigiiCampus / CollPoll. One binary, one token, 16 tools — and it plugs into every agent you run.**

[![CI](https://github.com/kushvinth/DigiMCP/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/kushvinth/DigiMCP/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/badge/crates.io-digimcp-f46623?style=flat-square&logo=rust&logoColor=white)](https://crates.io/crates/digimcp)
[![MCP server](https://img.shields.io/badge/MCP-16_tools-5c4ee5?style=flat-square)](#the-16-tools)
[![MIT license](https://img.shields.io/badge/license-MIT-f4c430?style=flat-square)](LICENSE)

[Install & forget](#install--forget) · [The 16 tools](#the-16-tools) · [Course Materials](#course-materials-notes--slides) · [Built-in RTK](#built-in-rtk-token-compaction-6080-savings) · [Zero Fiction & Journal](#zero-fiction-safety--audit-journal) · [Works in your agent](#works-in-your-agent) · [Dashboard](#interactive-cli--dashboard) · [Visual Themes](#visual-themes)

<br/>

<img src="assets/screenshots/digimcp-dashboard.png" alt="DigiMCP terminal dashboard preview" width="760"/>

<br/>

```bash
digi login && digi setup
```

</div>

---

Every student portal answers the same three questions badly: *what's due*,
*how much can I still bunk*, and *what did I get*. You click through nested menus,
fight slow pagination, and miss deadlines buried in course tabs.

**DigiMCP** connects your agent directly to the portal's internal API. Zero browser
overhead, cold startup under 6 ms, exact IANA timezone timestamps, in-tree RTK
token compaction to keep agent context lean, and 16 native tools for reading records,
browsing course materials, and executing validated coursework submissions.

---

## Install & forget

**macOS and Linux (Homebrew):**
```bash
brew install kushvinth/digimcp/digimcp
```
*(or via cargo)*
```bash
cargo install digimcp
```

**Linux, macOS or WSL (one-line installer):**
```bash
curl -fsSL https://raw.githubusercontent.com/kushvinth/DigiMCP/main/scripts/install.sh | sh
```

**Windows (pre-built binary or cargo):**
```powershell
# Pre-built Windows x86_64 binaries available on GitHub Releases:
# https://github.com/kushvinth/DigiMCP/releases/latest

# Or install via cargo:
cargo install digimcp
```

### Then plug it into your agent

```bash
# 1. Connect to your college portal (auto-detects session token from your browser)
digi login --portal <yourcollege>.digiicampus.com

# 2. Wire into Claude Code, Codex, Cursor, Gemini CLI, OpenCode
digi setup
```

<p align="center">
  <img src="assets/videos/digimcp-setup.gif" alt="Wiring digi into agents with digi setup" width="760"/>
</p>

Restart your agent. That's it: when it asks *"what's due this week?"*, `digi` answers in
6 ms with timezone-exact deadlines, attendance with bunk predictions, course slides, grades, or
submits feedback forms for you. Nothing to learn, nothing to call, and upgrading `digi`
never changes this.

> **Command names**: The primary CLI binary is **`digi`** (fast and intuitive). Binary aliases
> **`digii`** and **`digimcp`** are also provided. The crate name on crates.io is **`digimcp`**.

[Pre-built binaries & release assets](https://github.com/kushvinth/DigiMCP/releases) · [Per-agent manual setup](#works-in-your-agent) · [Dry-run previews](#agentic-actions--dry-run-preview)

---

## Built-in RTK token compaction (60–80% savings)

Student portals return enormous JSON payloads packed with raw HTML descriptions,
redundant navigation fields, inline base64 images, and empty keys. DigiMCP features an
in-tree **RTK-style token compactor** (`src/simplify.rs`) that runs directly in
memory before returning results to your agent:

- **HTML stripping**: Compresses verbose rich-text descriptions down to concise clean markdown/prose.
- **URI neutralization**: Replaces multi-kilobyte data URIs with compact placeholders.
- **Whitespace normalization**: Eliminates wasted runs of indentation and spaces.
- **Dead-field pruning**: Drops nulls and uninformative keys while preserving lists.

Your agent gets 100% of the relevant data while saving 60% to 80% of context window
tokens on every single tool call — with zero external dependencies or background daemons.

---

## The 16 tools

You almost never need to look up an ID. Pass human names like `class: "Physics"` or
`assignment: "oscillations"` and `digi` resolves them automatically. Both `snake_case`
and `camelCase` argument keys are supported seamlessly.

### Intelligence & Academics (Read)

| Tool | What it does |
|---|---|
| `due` | Merged list of upcoming assignments, quizzes, and exams sorted by urgency. Flags overdue work and includes `hoursUntilDue`. |
| `attendance` | Overall and per-course percentages, plus **`bunksLeft`** (classes you can skip while staying $\ge 75\%$) and **`bunksNeeded`** (classes needed to recover). Courses with zero total lessons correctly report `null` and `status: "no-data"`, and unmarked lessons are tracked with safety warnings if $>20\%$. |
| `materials` | All course study materials (PDFs, presentations, syllabus, problem sets, and web links) grouped by academic module. |
| `material` | Fetches a fresh, temporary signed CDN download URL for any specific course resource. |
| `grades` | Published results and component marks per subject. Unpublished items report `published: false` rather than guessing. |
| `assignment` | Deep dive on one assignment: description, due date, submission status, grades, remarks, and fresh signed download links. |
| `assignments` | Class-by-class course view: upcoming, previous, and draft items. |
| `classes` | All enrolled courses, sections, and component IDs (lectures, labs, tutorials). |
| `search` | Full-text search across all assignment titles and descriptions. |
| `account` | Session status: student name, roll number, college portal origin, and days until cookie expiry. |
| `feedback_forms` | Active faculty and course evaluation questionnaires with submission status. Filters out already-submitted forms by default. |
| `quiz_questions` | Questions and options for an active quiz so you can inspect them before answering. |

### Agentic Actions & Submissions (Write)

Action tools perform live portal operations by default and include full support for
`dry_run: true` so you can validate inputs and compliance before submitting.

| Tool | What it does |
|---|---|
| `submit_assignment` | Submit an assignment with answer text and optional attachment links. |
| `submit_quiz` | Submit answers for an active portal quiz. |
| `fill_feedback` | Complete faculty/course feedback questionnaires with verified rating coverage and constructive remarks. |
| `fill_form` | Generic campus survey and questionnaire submitter. |

---

## Zero-Fiction Safety & Audit Journal

Submitting coursework or faculty evaluations under your name must be bulletproof. DigiMCP enforces
three core safety guarantees:

1. **Dry-Run Live Validation**: `dry_run: true` fetches the live dynamic questionnaire template,
   inspects every section, and validates that 100% of required questions are answered. An empty
   or partial submission is immediately rejected with a detailed diff of missing items.
2. **Zero Fiction**: Missing ratings **never** default to 5.0 (which would give unrated faculty
   perfect scores), and subjective feedback is **never** fabricated.
3. **Append-Only Write Journal**: Every write action (`submit_assignment`, `submit_quiz`, `fill_feedback`,
   `fill_form`) is logged with an idempotency key to `~/.local/share/digimcp/journal.jsonl`.
   Retried tool calls or agent crashes cannot produce duplicate submissions without explicit `force: true` or `resubmit: true`.

#### Agentic actions & dry-run preview

To preview any submission without sending it to the portal, simply set `dry_run: true`:

```json
{
  "form": "CS3803",
  "ratings": [
    { "question_id": 101, "rating": 5, "remarks": "Clear problem breakdowns and responsive office hours." },
    { "question_id": 102, "rating": 4, "remarks": "Course pace is steady; assignments reinforce lecture topics well." }
  ],
  "dry_run": true
}
```

DigiMCP returns the validated coverage, total mandatory questions verified, resolved template IDs,
and the preview confirmation without mutating live portal state.

---

## Works in your agent

`digi setup` inspects your machine and safely configures these agents automatically.
If you prefer manual configuration, use the snippets below:

### Claude Code

```bash
# Register tool via CLI
claude mcp add --scope user digimcp -- digi

# Or install the official plugin (skills + MCP server in one command)
claude plugin marketplace add kushvinth/DigiMCP
claude plugin install digimcp@digimcp
```

### Codex

Add to `~/.codex/config.toml`:

```toml
[mcp_servers.digimcp]
command = "digi"
args = []
```

### Cursor

Add to `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "digimcp": {
      "command": "digi",
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
    "digimcp": {
      "command": "digi",
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
    "digimcp": {
      "type": "local",
      "command": ["digi"],
      "enabled": true
    }
  }
}
```

### Any other MCP host

DigiMCP is a standard MCP stdio server with no extra arguments required:

```json
{
  "command": "digi",
  "args": []
}
```

---

## Interactive CLI & Dashboard

Besides serving MCP over stdio, `digi` is a full student terminal utility:

<p align="center">
  <img src="assets/videos/digimcp-usage.gif" alt="Using digi CLI & diagnostics" width="760"/>
</p>

### Login & auto-detection

```bash
# Interactive browser sign-in (auto-detects session token from Zen, Chrome, Arc, Brave, Firefox, Edge, etc.)
digi login --portal snuc.digiicampus.com

# Direct token login
digi login --token eyJhbGci...

# Connect via Chrome DevTools Protocol
digi login --connect
```

### System doctor

```bash
digi doctor [--json]
```
Verifies token validity, portal connectivity, server clock drift, and active course enrollments.

<p align="center">
  <img src="assets/screenshots/digimcp-doctor.png" alt="digi doctor diagnostics check" width="760"/>
</p>

### Terminal dashboard

```bash
digi dashboard [--json] [--theme <name>]
```
Renders a clean two-column student dashboard featuring student session credentials, token expiration,
connected agents, and active capabilities.

### Visual themes

DigiMCP features four distinct visual themes engineered for high-contrast legibility without sloppy blue styling:

| Theme | Identifier | Aesthetic & Palette |
|---|---|---|
| **Pure Black** *(Default)* | `pure-black` | Pitch black (`#000000`) canvas with crisp neutral silver (`#9ca3af`) and bright white accents. |
| **Tokyo Night / Midnight** | `tokyo-night` | Deep indigo (`#1a1b26`) backdrop with neon purple (`#bb9af7`), soft storm blue (`#7aa2f7`), and coral. |
| **Crimson Dark / Digii** | `crimson-dark` | Deep obsidian crimson (`#0f0507`) with official DigiiCampus brand crimson (`#a0141e`) and scarlet. |
| **Cyberpunk / Matrix Emerald** | `matrix-emerald` | Phosphor matrix black (`#050d08`) with radiant emerald (`#10b981`) and electric mint (`#34d399`). |

```bash
# Instant preview via CLI argument
digi dashboard --theme pure-black
digi dashboard --theme tokyo-night
digi dashboard --theme crimson-dark
digi dashboard --theme matrix-emerald

# Or configure system-wide via environment variable
export DIGIMCP_THEME="pure-black"   # tokyo-night, crimson-dark, matrix-emerald

# Or save permanently in ~/.config/digimcp/config.toml
theme = "pure-black"
```

#### Theme previews

<table>
  <tr>
    <td width="50%">
      <p align="center"><b>Pure Black</b> (Default)</p>
      <img src="assets/screenshots/theme-pure-black.png" alt="Pure Black theme preview" width="100%"/>
    </td>
    <td width="50%">
      <p align="center"><b>Tokyo Night / Midnight</b></p>
      <img src="assets/screenshots/theme-tokyo-night.png" alt="Tokyo Night theme preview" width="100%"/>
    </td>
  </tr>
  <tr>
    <td width="50%">
      <p align="center"><b>Crimson Dark / Digii</b></p>
      <img src="assets/screenshots/theme-crimson-dark.png" alt="Crimson Dark theme preview" width="100%"/>
    </td>
    <td width="50%">
      <p align="center"><b>Cyberpunk / Matrix Emerald</b></p>
      <img src="assets/screenshots/theme-matrix-emerald.png" alt="Cyberpunk Matrix Emerald theme preview" width="100%"/>
    </td>
  </tr>
</table>

---

## Configuration & environment

Tokens are saved locally with strict filesystem permissions (`chmod 600`) at
`~/.config/digimcp/config.toml`. You can also configure DigiMCP via environment variables or a
`.env` file in your workspace:

| Variable | Purpose | Default |
|---|---|---|
| `DIGIMCP_THEME` | Visual theme (`pure-black`, `tokyo-night`, `crimson-dark`, `matrix-emerald`) | `pure-black` |
| `DIGIMCP_TOKEN` | Portal session token (`user` cookie starting with `eyJ`; accepts `QUAD_TOKEN`) | From `config.toml` |
| `DIGIMCP_BASE_URL` | Portal origin (e.g. `https://snuc.digiicampus.com`) | Derived from JWT |
| `DIGIMCP_TIMEZONE` | College IANA timezone | `Asia/Kolkata` |
| `DIGIMCP_TIMEOUT_MS` | Per-request network timeout | `40000` |
| `DIGIMCP_CONFIG_DIR` | Custom directory for config file storage | `~/.config/digimcp` |
| `DIGIMCP_DATA_DIR` | Custom directory for write action audit journal | `~/.local/share/digimcp` |

---

## Privacy & safety

- **Zero telemetry**: No analytics, tracking, or telemetry pings. Network requests only go to your college's portal.
- **Token stays local**: Tokens are stored with `chmod 600` permissions and sent solely in the `Auth-Token` header to your portal origin.
- **Automatic credential redaction**: Tokens, auth signatures, and secret query parameters are stripped before surfacing in agent context or error prose.
- **Accurate timezones**: Deadlines are mapped to your institution's IANA timezone and formatted in RFC 3339, eliminating cross-timezone deadline confusion.
- **Controlled write operations**: Write operations require coverage validation, support `dry_run: true` preview mode, and log to an audit journal.

---

## Development

```bash
# Run unit & e2e test suite (against mock portal)
cargo test --all-targets

# Linter & formatter
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check

# Validate ecosystem manifests
./scripts/validate-manifests.sh

# Build release binary
cargo build --release
```

---

## License

[MIT](LICENSE). Not affiliated with Digiicampus or CollPoll.
