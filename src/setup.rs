//! `quad setup` — register the MCP server and install the skill in every agent
//! on this machine.
//!
//! Design rules that keep this safe to run:
//!   * show every change before making it, apply only on approval;
//!   * back up each file it edits, then verify the edit parsed;
//!   * never rewrite a config it cannot parse (JSONC with comments, odd TOML);
//!     it prints the snippet to paste instead;
//!   * only ever touch quad's own entry, so `--remove` is exact.

use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

const SERVER_NAME: &str = "quad";

/// The skill text dropped into each agent's skills directory.
pub const SKILL: &str = include_str!("../skills/quad/SKILL.md");

pub fn run(args: Vec<String>) -> Result<(), BoxError> {
    let mut only: Option<Vec<String>> = None;
    let mut yes = false;
    let mut remove = false;

    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--agents" | "-a" => {
                only = it.next().map(|list| {
                    list.split(',')
                        .map(|s| s.trim().to_lowercase())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
            }
            "--yes" | "-y" => yes = true,
            "--remove" | "--uninstall" => remove = true,
            "--help" | "-h" => {
                println!(
                    "usage: quad setup [--agents claude,codex,cursor,gemini,opencode] [--yes] [--remove]"
                );
                return Ok(());
            }
            other => {
                eprintln!("quad setup: unknown option {other:?}");
                std::process::exit(2);
            }
        }
    }

    let exe = std::env::current_exe()?;
    let exe = exe.to_string_lossy().to_string();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();

    let agents: Vec<Agent> = detect(&home)
        .into_iter()
        .filter(|a| {
            only.as_ref()
                .is_none_or(|want| want.iter().any(|w| w == a.id))
        })
        .collect();

    if agents.is_empty() {
        println!("No supported agent found on this machine.");
        println!("Supported: Claude Code, Codex, Cursor, Gemini CLI, opencode.");
        print_manual(&exe);
        return Ok(());
    }

    let steps: Vec<(usize, &Step)> = agents
        .iter()
        .enumerate()
        .flat_map(|(i, a)| a.steps.iter().map(move |s| (i, s)))
        .collect();

    let verb = if remove { "remove" } else { "change" };
    println!(
        "quad setup: about to {verb} {} item(s) across {} agent(s):\n",
        steps.len(),
        agents.len()
    );
    for (i, step) in &steps {
        println!("  {:<9} {}", agents[*i].name, describe(step, remove));
    }
    println!();

    if !yes && !confirm(remove) {
        println!("Nothing changed.");
        return Ok(());
    }

    let mut failures = 0;
    for (i, step) in &steps {
        match if remove { undo(step) } else { apply(step) } {
            Ok(msg) => println!("  {:<9} {msg}", agents[*i].name),
            Err(e) => {
                failures += 1;
                println!("  {:<9} ✗ {e}", agents[*i].name);
            }
        }
    }

    println!();
    if remove {
        println!("Done. Restart your agent; your other servers and the saved token are untouched.");
    } else {
        println!("Done. Restart your agent, then ask it something like:");
        println!("  \"what's due this week?\"   ·   \"where do I stand on attendance?\"");
        println!("\nIf quad isn't connected yet, run `quad login` and then `quad doctor`.");
    }
    if failures > 0 {
        println!(
            "\n{failures} item(s) failed — see above. Paste the printed snippet by hand if needed."
        );
    }
    Ok(())
}

fn confirm(remove: bool) -> bool {
    print!("{}? [y/N] ", if remove { "Remove" } else { "Apply" });
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

fn print_manual(exe: &str) {
    println!("\nRegister it by hand with one of:\n");
    println!("  claude mcp add --scope user {SERVER_NAME} -- {exe}");
    println!("  # opencode: add to ~/.config/opencode/opencode.json");
    println!(
        "  \"{SERVER_NAME}\": {{ \"type\": \"local\", \"command\": [\"{exe}\"], \"enabled\": true }}"
    );
}

// ------------------------------------------------------------------------- //
// Agents and their steps
// ------------------------------------------------------------------------- //

#[derive(Debug, Clone)]
enum Step {
    /// Run an agent's own CLI (`claude mcp add ...`), with the inverse for --remove.
    Cmd {
        argv: Vec<String>,
        undo: Vec<String>,
        needs: String,
    },
    /// Copy SKILL.md into a skills directory.
    Skill { dir: PathBuf },
    /// Merge an entry into a JSON config.
    Json {
        file: PathBuf,
        /// Keys from the root to the map holding servers, e.g. ["mcp"].
        path: Vec<String>,
        entry: serde_json::Value,
    },
    /// Append a `[table]` block to a TOML file.
    Toml {
        file: PathBuf,
        table: String,
        body: String,
    },
}

struct Agent {
    id: &'static str,
    name: &'static str,
    steps: Vec<Step>,
}

fn on_path(cmd: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {cmd} >/dev/null 2>&1"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn detect(home: &Path) -> Vec<Agent> {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "quad".into());
    let mut out = Vec::new();

    // Claude Code — let its own CLI own ~/.claude.json.
    if on_path("claude") || home.join(".claude").exists() {
        out.push(Agent {
            id: "claude",
            name: "Claude Code",
            steps: vec![
                Step::Cmd {
                    argv: vec![
                        "claude".into(),
                        "mcp".into(),
                        "add".into(),
                        "--scope".into(),
                        "user".into(),
                        SERVER_NAME.into(),
                        "--".into(),
                        exe.clone(),
                    ],
                    undo: vec![
                        "claude".into(),
                        "mcp".into(),
                        "remove".into(),
                        "--scope".into(),
                        "user".into(),
                        SERVER_NAME.into(),
                    ],
                    needs: "claude".into(),
                },
                Step::Skill {
                    dir: home.join(".claude").join("skills").join(SERVER_NAME),
                },
            ],
        });
    }

    // Codex
    if on_path("codex") || home.join(".codex").exists() {
        out.push(Agent {
            id: "codex",
            name: "Codex",
            steps: vec![
                Step::Toml {
                    file: home.join(".codex").join("config.toml"),
                    table: format!("mcp_servers.{SERVER_NAME}"),
                    body: format!("command = {}\nargs = []\n", toml_string(&exe)),
                },
                Step::Skill {
                    dir: home.join(".codex").join("skills").join(SERVER_NAME),
                },
            ],
        });
    }

    // Cursor
    if home.join(".cursor").exists() {
        out.push(Agent {
            id: "cursor",
            name: "Cursor",
            steps: vec![
                Step::Json {
                    file: home.join(".cursor").join("mcp.json"),
                    path: vec!["mcpServers".into()],
                    entry: serde_json::json!({ "command": exe, "args": [] }),
                },
                Step::Skill {
                    dir: home.join(".cursor").join("skills").join(SERVER_NAME),
                },
            ],
        });
    }

    // Gemini CLI
    if on_path("gemini") || home.join(".gemini").exists() {
        out.push(Agent {
            id: "gemini",
            name: "Gemini CLI",
            steps: vec![Step::Json {
                file: home.join(".gemini").join("settings.json"),
                path: vec!["mcpServers".into()],
                entry: serde_json::json!({ "command": exe, "args": [] }),
            }],
        });
    }

    // opencode — one server key, plus a skill directory.
    if on_path("opencode") || home.join(".config").join("opencode").exists() {
        out.push(Agent {
            id: "opencode",
            name: "opencode",
            steps: vec![
                Step::Json {
                    file: home.join(".config").join("opencode").join("opencode.json"),
                    path: vec!["mcp".into()],
                    entry: serde_json::json!({
                        "type": "local",
                        "command": [exe],
                        "enabled": true
                    }),
                },
                Step::Skill {
                    dir: home
                        .join(".config")
                        .join("opencode")
                        .join("skills")
                        .join(SERVER_NAME),
                },
            ],
        });
    }

    out
}

fn toml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn describe(step: &Step, remove: bool) -> String {
    match step {
        Step::Cmd { argv, undo, .. } => {
            if remove {
                format!("run     {}", undo.join(" "))
            } else {
                format!("run     {}", argv.join(" "))
            }
        }
        Step::Skill { dir } => {
            if remove {
                format!("delete  {}", dir.display())
            } else {
                format!("skill   {}", dir.join("SKILL.md").display())
            }
        }
        Step::Json { file, .. } => {
            if remove {
                format!("unset   {}  [\"{SERVER_NAME}\"]", file.display())
            } else {
                format!("edit    {}  [\"{SERVER_NAME}\"]", file.display())
            }
        }
        Step::Toml { file, table, .. } => {
            if remove {
                format!("unset   {}  [{table}]", file.display())
            } else {
                format!("edit    {}  [{table}]", file.display())
            }
        }
    }
}

fn apply(step: &Step) -> Result<String, BoxError> {
    match step {
        Step::Cmd { argv, needs, .. } => {
            if !on_path(needs) {
                return Ok(format!("skipped ({needs} not on PATH)"));
            }
            let out = Command::new(&argv[0]).args(&argv[1..]).output()?;
            if out.status.success() {
                Ok("registered".into())
            } else {
                let err = String::from_utf8_lossy(&out.stderr);
                let out_s = String::from_utf8_lossy(&out.stdout);
                Err(first_line(&err)
                    .or_else(|| first_line(&out_s))
                    .unwrap_or_else(|| "failed".into())
                    .into())
            }
        }
        Step::Skill { dir } => {
            fs::create_dir_all(dir)?;
            let path = dir.join("SKILL.md");
            fs::write(&path, SKILL)?;
            Ok(format!("installed {}", path.display()))
        }
        Step::Json { file, path, entry } => {
            let original = fs::read_to_string(file).ok();
            let text = original.clone().unwrap_or_else(|| "{}".into());
            let mut root: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => {
                    print_json_snippet(file, path, entry);
                    return Err(format!(
                        "{} is not plain JSON (comments?) — left untouched. Add the snippet printed above by hand, or let your agent do it.",
                        file.display()
                    )
                    .into());
                }
            };
            let mut cursor = &mut root;
            for key in path {
                let obj = cursor
                    .as_object_mut()
                    .ok_or_else(|| format!("{} is not a JSON object", file.display()))?;
                cursor = obj
                    .entry(key.clone())
                    .or_insert_with(|| serde_json::json!({}));
            }
            let obj = cursor.as_object_mut().ok_or_else(|| {
                format!("{} has a non-object at {}", file.display(), path.join("."))
            })?;
            obj.insert(SERVER_NAME.into(), entry.clone());
            write_verified(file, original.as_deref(), &root)
        }
        Step::Toml { file, table, body } => {
            let original = fs::read_to_string(file).ok();
            let text = original.clone().unwrap_or_default();
            if text.lines().any(|l| l.trim() == format!("[{table}]")) {
                return Ok("already registered".into());
            }
            let sep = if text.is_empty() {
                ""
            } else if text.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            let next = format!("{text}{sep}[{table}]\n{body}");
            fs::create_dir_all(file.parent().unwrap_or(Path::new(".")))?;
            backup(file, original.as_deref())?;
            fs::write(file, next)?;
            Ok(format!("updated {}", file.display()))
        }
    }
}

fn undo(step: &Step) -> Result<String, BoxError> {
    match step {
        Step::Cmd { undo, needs, .. } => {
            if !on_path(needs) {
                return Ok(format!("skipped ({needs} not on PATH)"));
            }
            let out = Command::new(&undo[0]).args(&undo[1..]).output()?;
            Ok(if out.status.success() {
                "unregistered".into()
            } else {
                "was not registered".to_string()
            })
        }
        Step::Skill { dir } => {
            let path = dir.join("SKILL.md");
            match fs::remove_file(&path) {
                Ok(()) => {
                    let _ = fs::remove_dir(dir);
                    Ok(format!("removed {}", path.display()))
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    Ok("was not installed".into())
                }
                Err(e) => Err(e.into()),
            }
        }
        Step::Json { file, path, .. } => {
            let Some(original) = fs::read_to_string(file).ok() else {
                return Ok("no file".into());
            };
            let Ok(mut root) = serde_json::from_str::<serde_json::Value>(&original) else {
                return Err(format!(
                    "{} is not plain JSON — remove the entry by hand",
                    file.display()
                )
                .into());
            };
            let mut cursor = &mut root;
            for key in path {
                match cursor.get_mut(key) {
                    Some(next) => cursor = next,
                    None => return Ok("was not registered".into()),
                }
            }
            let removed = cursor
                .as_object_mut()
                .and_then(|o| o.remove(SERVER_NAME))
                .is_some();
            if !removed {
                return Ok("was not registered".into());
            }
            write_verified(file, Some(&original), &root)
        }
        Step::Toml { file, table, .. } => {
            let Some(original) = fs::read_to_string(file).ok() else {
                return Ok("no file".into());
            };
            let header = format!("[{table}]");
            let mut kept = Vec::new();
            let mut skipping = false;
            for line in original.lines() {
                if line.trim() == header {
                    skipping = true;
                    continue;
                }
                if skipping && line.trim_start().starts_with('[') {
                    skipping = false;
                }
                if !skipping {
                    kept.push(line);
                }
            }
            let next = kept.join("\n");
            backup(file, Some(&original))?;
            fs::write(
                file,
                if next.trim().is_empty() {
                    String::new()
                } else {
                    format!("{next}\n")
                },
            )?;
            Ok(format!("updated {}", file.display()))
        }
    }
}

fn write_verified(
    file: &Path,
    original: Option<&str>,
    root: &serde_json::Value,
) -> Result<String, BoxError> {
    let body = format!("{}\n", serde_json::to_string_pretty(root)?);
    // Never write a file we could not read back.
    serde_json::from_str::<serde_json::Value>(&body)?;
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let saved = backup(file, original)?;
    fs::write(file, body)?;
    Ok(match saved {
        Some(name) => format!("updated {} (backup {name})", file.display()),
        None => format!("created {}", file.display()),
    })
}

fn backup(file: &Path, original: Option<&str>) -> Result<Option<String>, BoxError> {
    let Some(text) = original else {
        return Ok(None);
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let name = format!(
        "{}.quad-backup-{stamp}",
        file.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    );
    let dst = file.with_file_name(name.clone());
    fs::write(&dst, text)?;
    Ok(Some(name))
}

fn print_json_snippet(file: &Path, path: &[String], entry: &serde_json::Value) {
    let closing = std::iter::repeat_n("}", path.len())
        .collect::<Vec<_>>()
        .join(", ");
    let nested: String = path.iter().map(|k| format!("\"{k}\": {{ ")).collect();
    let body = serde_json::to_string_pretty(entry)
        .unwrap_or_default()
        .lines()
        .collect::<Vec<_>>()
        .join("\n  ");
    println!(
        "\n  Add this to {}:\n\n  {nested}\"{SERVER_NAME}\": {body}{closing}\n",
        file.display()
    );
}

fn first_line(s: &str) -> Option<String> {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

#[allow(dead_code)]
fn config_path() -> Option<PathBuf> {
    config::config_file()
}
