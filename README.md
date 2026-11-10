# fijii-mcp

A **read-only** [MCP](https://modelcontextprotocol.io) server that exposes college data
(classes, assignments, attendance, coursework, grades, quizzes, discussions) to an Agent, built on the
[`fijii`](https://github.com/guruprasadah/fijii) SDK for the Digiicampus student portal.

Runs locally over **stdio**. One student per instance. No write operations are exposed.

## Tools

| Tool | What it answers |
|---|---|
| `list_classes` | Every class / course component you're enrolled in |
| `find_class` | Resolve a course name or code to a `classId` |
| `pending_assignments` | Everything still due, across all classes, earliest first |
| `list_assignments` | Upcoming / previous / draft assignments for one class |
| `get_assignment` | Full detail: description, resources, submissions, rubric |
| `list_terms` | Academic terms (for the attendance `termId`) |
| `get_attendance` | Term totals + per-course/component breakdown |
| `get_gradebook` | Your own assessment result for a class |
| `list_coursework` | Modules, sessions and resources for a class |
| `list_quizzes` | Quizzes for a class (provisional) |
| `list_discussions` | Class forum threads |
| `get_discussion` | One discussion thread |
| `institution_info` | College metadata, server clock, feature flags |
| `get_profile` | Your profile (contact fields opt-in) |

Every failure comes back as an `isError` tool result with an actionable message (e.g. "Session expired —
refresh `DIGII_TOKEN`"), never a crash.

## Setup

The SDK is vendored as a git submodule and is built automatically on install:

```bash
git clone --recurse-submodules <this-repo> fijii-mcp
cd fijii-mcp
npm install      # postinstall builds vendor/fijii, then links it
npm run build
```

If you already cloned without submodules: `git submodule update --init --recursive`, then `npm install`.
(The `postinstall` step skips quietly if the submodule is absent, and rebuilds only when
`vendor/fijii/dist` is missing.)

### Credentials

Two environment variables are required:

| Variable | Purpose |
|---|---|
| `DIGII_TOKEN` | The JWT — the `user` cookie from the logged-in portal (DevTools → Application → Cookies → `user`) |
| `DIGII_BASE_URL` | Your institution origin, e.g. `https://<your-institute>.digiicampus.com` |

Optional: `DIGII_TIMEZONE` (default `Asia/Kolkata`), `DIGII_TIMEOUT_MS` (default `40000`).

The token is **re-read from the environment on every request**, so rotating it takes effect without
restarting the server.

> Never commit a token. `.env`, `*.har` and real credentials are git-ignored.

### Client configuration

**Claude Desktop** (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "fijii": {
      "command": "node",
      "args": ["/absolute/path/to/fijii-mcp/dist/index.js"],
      "env": {
        "DIGII_TOKEN": "eyJhbGciOi...",
        "DIGII_BASE_URL": "https://your-institute.digiicampus.com"
      }
    }
  }
}
```

**OpenCode** (`opencode.jsonc` in the project, or global `~/.config/opencode/opencode.jsonc`):

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "fijii": {
        "type": "local",
        "command": ["node", "/absolute/path/to/fijii-mcp/dist/index.js"],
        "environment": {
          "DIGII_TOKEN": "{env:DIGII_TOKEN}",
          "DIGII_BASE_URL": "{env:DIGII_BASE_URL}"
        }
      }
    }
  }
}
```

Using `{env:DIGII_TOKEN}` keeps the secret out of the file; export it in the shell that launches OpenCode
(or substitute the literal value). Because the token is re-read on every request, updating the exported
variable's source and restarting OpenCode picks up a rotated token. Verify with `opencode mcp list`
(expect `✓ fijii  connected`).

## Testing

```bash
npm run typecheck     # tsc --noEmit
npm run smoke         # spawn over stdio, handshake, list tools
node test/error-path.js
```

With `DIGII_TOKEN` set, `npm run smoke` also calls `institution_info`, `get_profile` and `list_classes`
live.

## Scope & caveats

- **Read-only by design.** No submissions, quiz attempts, discussion posts or attendance exceptions.
- The portal API is private and undocumented; endpoints can change without notice.
- Media URLs are signed and expire in roughly an hour.
- Quiz and discussion data may be empty on some accounts (`provisional` in the SDK's terms).
- Use only accounts you are authorised to use, and respect your institution's terms. Attendance,
  grades and enrolment are personal data.

Not affiliated with Digiicampus or CollPoll. MIT licensed.
