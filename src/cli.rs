//! CLI commands and MCP runtime entry points for DigiMCP.

use rmcp::ServiceExt;
use rmcp::transport::stdio;
use std::io::IsTerminal;

use crate::config;
use crate::portal;
use crate::server::Quad;
use crate::ui;
use crate::{login, setup};

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub fn real_main() -> Result<(), BoxError> {
    let mut args = std::env::args().skip(1);
    let command = args.next();

    match command.as_deref() {
        // Agents launch us with no arguments: serve MCP over stdio.
        None => {
            if std::io::stdin().is_terminal() {
                ui::info(&format!(
                    "DigiMCP v{} — serving MCP over stdio. Press Ctrl+C to stop.",
                    env!("CARGO_PKG_VERSION")
                ));
                ui::step(
                    "Run `digi dashboard` for overview, `digi doctor` for diagnostics, or `digi --help` for commands.\n",
                );
            }
            serve()
        }
        Some("serve") | Some("mcp") => serve(),
        Some("dashboard") | Some("status") | Some("banner") => {
            let rest: Vec<String> = args.collect();
            let json_mode = rest.iter().any(|a| a == "--json");
            let theme = ui::Theme::apply_args(&rest);
            let cfg = config::load(None).ok();
            if json_mode {
                let j = serde_json::json!({
                    "version": env!("CARGO_PKG_VERSION"),
                    "configured": cfg.is_some(),
                    "expired": cfg.as_ref().map(|c| c.is_expired()).unwrap_or(true),
                    "daysLeft": cfg.as_ref().and_then(|c| c.days_left()),
                    "college": cfg.as_ref().and_then(|c| c.college.clone()),
                    "origin": cfg.as_ref().map(|c| c.base_url.clone()),
                    "timezone": cfg.as_ref().map(|c| c.timezone.clone()),
                    "theme": theme.name(),
                });
                println!("{}", serde_json::to_string_pretty(&j)?);
            } else if command.as_deref() == Some("banner") {
                ui::print_banner_themed(theme);
            } else {
                ui::print_dashboard_themed(cfg.as_ref(), theme);
            }
            Ok(())
        }
        Some("--help") | Some("-h") | Some("help") => {
            print!("{}", HELP);
            Ok(())
        }
        Some("--version") | Some("-V") | Some("version") => {
            println!("digi {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("login") => login::run(args.collect()),
        Some("logout") => login::logout(),
        Some("setup") => setup::run(args.collect()),
        Some("doctor") => {
            let rest: Vec<String> = args.collect();
            let json_mode = rest.iter().any(|a| a == "--json");
            let _ = ui::Theme::apply_args(&rest);
            doctor(json_mode)
        }
        Some(other) => {
            eprintln!("digi: unknown command {other:?}\n\n{HELP}");
            std::process::exit(2);
        }
    }
}

pub const HELP: &str = "\
DigiMCP (digi) — what is due, course materials, attendance, and agentic submissions.

A complete MCP server and student CLI for DigiiCampus (CollPoll) student portals.

USAGE
  digi                      serve MCP over stdio (what agents run)
  digi login                open portal, detect session token from browser, and save
       --portal <host>        your institution's portal host (e.g. snuc.digiicampus.com)
       --token <jwt>          paste a token directly instead of using a browser
       --manual               paste token manually from browser DevTools
  digi setup                register digi in every agent found (Claude, Codex, Cursor, Gemini, OpenCode)
       --agents a,b           limit to specific agents
       --yes                  skip confirmation prompts
       --remove               undo configuration
  digi doctor [--json] [--theme <t>]    check credentials, portal reachability and configuration
  digi dashboard [--json] [--theme <t>] show student identity, session status and core capabilities
       Themes: pure-black (default), tokyo-night, crimson-dark, matrix-emerald
  digi logout                           delete the saved token
  digi version | help

ENVIRONMENT
  DIGIMCP_THEME       dashboard theme: pure-black (default), tokyo-night, crimson-dark, matrix-emerald
  DIGIMCP_TOKEN       session token (overrides saved config; also accepts QUAD_TOKEN)
  DIGIMCP_BASE_URL    portal origin, e.g. https://<institute>.digiicampus.com
  DIGIMCP_TIMEZONE    college timezone, e.g. Asia/Kolkata
  DIGIMCP_TIMEOUT_MS  per-request network timeout (ms), default 40000
  DIGIMCP_CONFIG_DIR  directory where config.toml lives, default ~/.config/digimcp
  DIGIMCP_DATA_DIR    directory where action audit journal lives, default ~/.local/share/digimcp

TOKEN COMPACTION (RTK)
  DigiMCP optimizes portal responses before returning them to the agent:
    • Compresses portal HTML and styling into clean markdown and prose
    • Truncates opaque base64 data URIs to avoid context pollution
    • Prunes redundant JSON nulls while preserving structural schemas
    • Reduces prompt context tokens across tool execution loops

CAPABILITIES (16 MCP TOOLS)
  Academics:   due, attendance, assignments, assignment, grades, courses, search
  Materials:   materials (module notes/slides), material (signed download links)
  Records:     classes, account, feedback_forms, quiz_questions
  Agentic:     submit_assignment, submit_quiz, fill_feedback, fill_form
               (All write actions support dry_run: true preview and journal audit)
";

pub fn serve() -> Result<(), BoxError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let server = Quad::new(config::load(None));
    runtime.block_on(async move {
        let running = server.clone().serve(stdio()).await?;
        eprintln!("digi {} ready", env!("CARGO_PKG_VERSION"));
        running.waiting().await?;
        Ok::<_, BoxError>(())
    })
}

pub fn doctor(json_mode: bool) -> Result<(), BoxError> {
    let cfg = match config::load(None) {
        Ok(cfg) => cfg,
        Err(err) => {
            if json_mode {
                let out = serde_json::json!({
                    "status": "error",
                    "error": err.to_string(),
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!();
                ui::step(&format!(
                    "DigiMCP doctor v{} — inspecting configuration and portal health",
                    env!("CARGO_PKG_VERSION")
                ));
                ui::err(&format!("Configuration: {err}"));
                println!("\n  Fix it with:  digi login --portal <institute>.digiicampus.com\n");
            }
            return Ok(());
        }
    };

    if !json_mode {
        println!();
        ui::step(&format!(
            "DigiMCP doctor v{} — inspecting configuration and portal health",
            env!("CARGO_PKG_VERSION")
        ));
        ui::ok(&format!(
            "Token found{}",
            cfg.college
                .as_ref()
                .map(|c| format!(" for {c}"))
                .unwrap_or_default()
        ));
        match cfg.expires_at {
            Some(_exp) => {
                let raw_days = cfg.days_left().unwrap_or(0);
                if cfg.is_expired() {
                    let days = (-raw_days).max(0);
                    let day_str = if days == 1 { "day" } else { "days" };
                    ui::err(&format!(
                        "Token expired {days} {day_str} ago — run `digi login` again"
                    ));
                } else {
                    let day_str = if raw_days == 1 { "day" } else { "days" };
                    ui::ok(&format!("Token valid ({raw_days} {day_str} left)"));
                }
            }
            None => ui::info("Token has no expiry claim"),
        }
        ui::info(&format!(
            "Portal origin: {}",
            if cfg.base_url.is_empty() {
                "(unknown)"
            } else {
                &cfg.base_url
            }
        ));
        ui::info(&format!("Timezone: {}", cfg.timezone));
        let dir = config::config_dir();
        if let Some(dir) = dir {
            ui::info(&format!(
                "Config: {} (mode {})",
                dir.join("config.toml").display(),
                mode_note()
            ));
        }
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let portal = portal::Portal::new(cfg.clone())?;
    runtime.block_on(async {
        match portal.me().await {
            Ok(me) => {
                let name = portal::clean(me.get("name"));
                let programme = portal::clean(me.get("programmeName"));
                let classes_count = portal.classes().await.map(|c| c.len()).unwrap_or(0);
                let (prog_id, batch_id) = portal.profile_ids().await.unwrap_or((0, 0));
                let terms_count = if prog_id > 0 {
                    portal
                        .terms(prog_id, batch_id)
                        .await
                        .map(|t| t.len())
                        .unwrap_or(0)
                } else {
                    0
                };

                if json_mode {
                    let out = serde_json::json!({
                        "status": "ok",
                        "name": name,
                        "programme": programme,
                        "college": cfg.college,
                        "classesCount": classes_count,
                        "termsCount": terms_count,
                        "tokenDaysLeft": cfg.days_left(),
                        "portal": cfg.base_url,
                        "timezone": cfg.timezone,
                    });
                    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
                } else {
                    ui::ok(&format!(
                        "Signed in as {name}{}",
                        if programme.is_empty() {
                            String::new()
                        } else {
                            format!(" ({programme})")
                        }
                    ));
                    let class_str = if classes_count == 1 { "class" } else { "classes" };
                    ui::ok(&format!("{classes_count} {class_str} visible"));
                    if prog_id > 0 {
                        let term_str = if terms_count == 1 { "term" } else { "terms" };
                        ui::ok(&format!(
                            "Programme {prog_id}, batch {batch_id} ({terms_count} {term_str} visible)"
                        ));
                    }
                    println!();
                    ui::ok("All checks passed. DigiMCP is ready for your agents.");
                    println!("  › Run `digi dashboard` to view your schedule & tool summary.\n");
                }
            }
            Err(e) => {
                if json_mode {
                    let out = serde_json::json!({
                        "status": "error",
                        "error": e.to_string(),
                    });
                    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
                } else {
                    ui::err(&format!("Portal: {e}"));
                    println!("\n  Session may have lapsed. Run `digi login` to refresh.\n");
                }
            }
        }
    });
    Ok(())
}

#[cfg(unix)]
fn mode_note() -> &'static str {
    "600"
}

#[cfg(not(unix))]
fn mode_note() -> &'static str {
    "default"
}
