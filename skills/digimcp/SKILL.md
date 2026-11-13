---
name: digimcp
description: Answer questions about a Digiicampus (CollPoll) college portal — what's due this week, course materials, assignment details, attendance percentages and how many classes can still be missed, grades, and agentic submissions. Use whenever the user asks about their college work, deadlines, submissions, attendance, marks, results, lecture notes, timetable-adjacent assignment load, or their student portal. Triggers: "what's due", "deadlines", "assignments", "course materials", "notes", "lecture slides", "did I submit", "my attendance", "how many classes can I bunk", "my marks", "results", "what do I have this week", "college", "portal".
license: MIT
compatibility: [claude, claude-code, opencode, codex, cursor, gemini]
---

# DigiMCP — DigiiCampus student portal interface

DigiMCP is an MCP server backed by the student's own session token. It reads
classes, assignments, quizzes, course materials, feedback forms, attendance and grades,
and provides agentic action tools for submitting assignments, attempting quizzes,
filling feedback forms, and completing campus surveys.

Answer portal questions from DigiMCP's tools, never from memory or guesses: the
student's record changes weekly and fabricated deadlines or marks are worse
than "I can't see that".

## Tool map — pick the one that matches the question

| The user asks | Call |
|---|---|
| "what's due", "what do I have this week", "anything overdue" | `due` (`within_hours: 24` for today, `168` for the week) |
| "what notes do I have", "lecture slides", "course materials" | `materials` (pass `class_name` or `class_id`) |
| "download note", "link for lab sheet" | `material` (`resource_id`) |
| "what's the assignment", "the description/attachments", "did I submit", "what did I get on it" | `assignment` with `assignment` name |
| "submit assignment", "turn in my lab report" | `submit_assignment` (optional `dry_run: true` to preview) |
| "what questions are on the quiz" | `quiz_questions` with the quiz name |
| "submit quiz answers", "attempt the quiz" | `submit_quiz` (optional `dry_run: true` to preview) |
| "pending feedback forms", "surveys to fill" | `feedback_forms` |
| "fill faculty feedback", "course evaluation" | `fill_feedback` (optional `dry_run: true` to validate coverage) |
| "submit campus form", "fill survey" | `fill_form` (optional `dry_run: true` to preview) |
| "my attendance", "how many can I bunk", "am I below 75" | `attendance` |
| "my marks", "my results", "how am I doing in Physics" | `grades` (pass a class name to narrow) |
| "which classes am I in", "what's my class code for X" | `classes` |
| "search my assignments for …" | `search` |
| anything looks broken, or the first call in a session | `account` |

You rarely need IDs: pass **class names** ("Physics", "DSA", "25CS201") and
DigiMCP resolves them. Pass **assignment names** the same way.

All tool argument schemas accept standard `snake_case` (e.g. `class_id`, `question_id`, `form_id`)
and support camelCase aliases (`classId`, `questionId`, `formId`).

## Write Operations & Strict Dry-Run Verification

Action tools (`submit_assignment`, `submit_quiz`, `fill_feedback`, `fill_form`)
perform live portal submissions by default. Pass `dry_run: true` whenever you want
to inspect the validated parameters:

- `fill_feedback` with `dry_run: true` fetches the live questionnaire template, verifies
  that all mandatory questions have supplied ratings and remarks, checks coverage,
  and reports any missing questions without submitting.
- Never invent fabricated praise or fake default ratings. Ratings and remarks must reflect
  the user's actual evaluation.
- Write actions record an entry in the local journal (`~/.local/share/digimcp/journal.jsonl`)
  for auditability and idempotency.

## Rules that make the answers right

1. **Times are RFC 3339 in the college timezone.** Quote them with the day name
   and local time ("Friday 14 Nov, 23:59 IST"), never a raw ISO string. Use
   `hoursUntilDue` for urgency: negative = overdue, < 24 = today/tomorrow.
2. **`due` merges assignments and quizzes**, sorted by deadline, with `kind` on
   each row. Report them together; the user thinks in "what's coming up".
3. **Overdue items stay in `due` by default.** If the user asks what's next,
   separate "overdue and still open" from "upcoming" — both matter.
4. **Attendance maths is DigiMCP's, the threshold is the college's.** `bunksLeft`
   and `bunksNeeded` assume 75%. Courses with zero held classes report `status: "no-data"`
   rather than a false 100%. `unmarkedLessons` indicates attendance uncertainty.
5. **`grades` marks unpublished marks honestly** (`published: false`). Never
   infer a grade from an assignment score unless the user asks for a projection,
   and label it as one.
6. **Attachments are signed links that expire in about an hour.** If a user
   revisits a link later, call `assignment` or `material` again for a fresh one.

## Setup — do it yourself, don't make the student read a manual

If a tool returns "No token found" or "Session expired":

1. Say one line: "DigiMCP needs your portal session — it takes a moment."
2. Ask for the `user` cookie value from the portal or run `digi login`.
3. Save it with `digi login --portal <host> --token <value>` (or `digi login --portal <host> --user <id> --pass <pwd>` on headless systems), then `digi doctor`.
4. Re-run the tool they actually wanted before replying.

Sessions expire (usually weeks). If `account` reports `daysLeft <= 0`, say the
token lapsed and ask for a fresh one.

## Guardrails

- The token grants full access to a student's record. Never print it, never
  paste it into a file, never send it anywhere but the portal.
- Personal data (grades, attendance) is theirs alone: don't publish it to the
  web, commit it, or include it in a public artifact without being asked.
