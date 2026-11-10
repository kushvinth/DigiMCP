//! `quad` — what is due, what is my attendance, what did I get.
//!
//! A read-only MCP server for a Digiicampus (CollPoll) student portal, plus the
//! setup commands that wire it into Claude Code, Codex, opencode and friends.

mod config;
mod error;
mod login;
mod portal;
mod registry;
mod server;
mod setup;
mod simplify;
mod tools;

use rmcp::ServiceExt;
use rmcp::transport::stdio;
use server::Quad;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

fn main() -> Result<(), BoxError> {
    let mut args = std::env::args().skip(1);
    let command = args.next();

    match command.as_deref() {
        // Agents launch us with no arguments: serve MCP over stdio.
        None | Some("serve") | Some("mcp") => serve(),
        Some("--help") | Some("-h") | Some("help") => {
            print!("{}", HELP);
            Ok(())
        }
        Some("--version") | Some("-V") | Some("version") => {
            println!("quad {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("login") => login::run(args.collect()),
        Some("logout") => login::logout(),
        Some("setup") => setup::run(args.collect()),
        Some("doctor") => doctor(),
        Some(other) => {
            eprintln!("quad: unknown command {other:?}\n\n{HELP}");
            std::process::exit(2);
        }
    }
}

const HELP: &str = "\
quad — what is due, what is my attendance, what did I get.

A read-only MCP server for a Digiicampus (CollPoll) student portal.

USAGE
  quad                      serve MCP over stdio (what agents run)
  quad login                open the portal, grab the session token, save it
       --portal <host>        your institution's portal host
       --token <jwt>          paste a token instead of using a browser
  quad setup                register quad + its skill in every agent found
       --agents a,b           limit to specific agents
       --yes                  skip confirmation prompts
       --remove               undo it
  quad doctor               check credentials, portal reachability and config
  quad logout               delete the saved token
  quad version | help

ENVIRONMENT
  QUAD_TOKEN        session token (overrides the saved config)
  QUAD_BASE_URL     portal origin, e.g. https://<institute>.digiicampus.com
  QUAD_TIMEZONE     college timezone, e.g. Asia/Kolkata (auto-detected when possible)
  QUAD_TIMEOUT_MS   per-request timeout, default 40000
  QUAD_CONFIG_DIR   where the token lives, default ~/.config/quad

Read-only by design: quad never submits work, answers quizzes or posts to forums.
";

fn serve() -> Result<(), BoxError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let server = Quad::new(config::load(None));
    runtime.block_on(async move {
        let running = server.clone().serve(stdio()).await?;
        eprintln!("quad {} ready", env!("CARGO_PKG_VERSION"));
        running.waiting().await?;
        Ok::<_, BoxError>(())
    })
}

fn doctor() -> Result<(), BoxError> {
    let cfg = match config::load(None) {
        Ok(cfg) => cfg,
        Err(err) => {
            println!("✗ configuration: {err}");
            println!("\nFix it with:  quad login --portal <institute>.digiicampus.com");
            return Ok(());
        }
    };
    println!(
        "✓ token found{}",
        cfg.college
            .as_ref()
            .map(|c| format!(" for {c}"))
            .unwrap_or_default()
    );
    match cfg.expires_at {
        Some(_exp) => {
            let days = cfg.days_left().unwrap_or(0);
            if cfg.is_expired() {
                println!("✗ token expired {days} day(s) ago — run `quad login` again");
            } else {
                println!("✓ token valid, {days} day(s) left");
            }
        }
        None => println!("• token has no expiry claim"),
    }
    println!(
        "• portal origin: {}",
        if cfg.base_url.is_empty() {
            "(unknown)"
        } else {
            &cfg.base_url
        }
    );
    println!("• timezone: {}", cfg.timezone);
    let dir = config::config_dir();
    if let Some(dir) = dir {
        println!(
            "• config: {} (mode {})",
            dir.join("config.toml").display(),
            mode_note()
        );
    }

    let runtime = tokio::runtime::Runtime::new()?;
    let portal = portal::Portal::new(cfg)?;
    runtime.block_on(async {
        match portal.me().await {
            Ok(me) => {
                let name = portal::clean(me.get("name"));
                let programme = portal::clean(me.get("programmeName"));
                println!(
                    "✓ signed in as {name}{}",
                    if programme.is_empty() {
                        String::new()
                    } else {
                        format!(" ({programme})")
                    }
                );
                match portal.classes().await {
                    Ok(classes) => println!("✓ {} class(es) visible", classes.len()),
                    Err(e) => println!("✗ classes: {e}"),
                }
                match portal.profile_ids().await {
                    Ok((p, b)) => {
                        println!("✓ programme {p}, batch {b}");
                        match portal.terms(p, b).await {
                            Ok(terms) => println!("✓ {} term(s) visible", terms.len()),
                            Err(e) => println!("✗ terms: {e}"),
                        }
                    }
                    Err(e) => println!("✗ profile: {e}"),
                }
            }
            Err(e) => println!("✗ portal: {e}"),
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
