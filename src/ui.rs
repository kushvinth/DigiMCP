//! TUI presentation: DigiiCampus red-to-white gradient FIGlet banner,
//! animatable braille spinners, clean status badges, and a rich Hermes-style
//! two-column terminal dashboard.
//!
//! Human engineering rationale:
//! CLI tools are often either plain and boring or overloaded with tacky emojis
//! that render with uneven widths across different terminal emulators (macOS
//! Terminal vs iTerm2 vs Alacritty vs Ghostty vs Windows Terminal).
//!
//! Here we adopt a clean, authoritative TUI style:
//!   * DigiiCampus brand crimson (#A0141E) to white gradient
//!   * Crisp Unicode box-drawing glyphs (`╭─╮`, `│`, `╰─╯`)
//!   * Uniform-width status markers (`✔`, `✖`, `›`, `•`)
//!   * A two-column dashboard (inspired by Hermes CLI) displaying student session
//!     identity on the left and available tools/agents on the right.

use std::io::{IsTerminal, Write, stdout};
use std::time::Instant;

use crate::config::Config;
use crate::error::jwt_claims;
use crate::portal::clean;

/// Check whether stdout is attached to an interactive terminal supporting ANSI color.
pub fn is_color_terminal() -> bool {
    stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

/// DigiiCampus brand color palette: Crimson (#A0141E) -> Scarlet (#D32F2F) -> Coral (#FF6F61) -> Warm White (#FFF5F5) -> Pure White (#FFFFFF)
const GRADIENT_STOPS: &[(u8, u8, u8)] = &[
    (160, 20, 30),   // Deep Crimson
    (211, 47, 47),   // DigiiCampus Red
    (239, 68, 68),   // Scarlet
    (255, 111, 97),  // Coral
    (255, 180, 180), // Soft Pinkish White
    (255, 230, 230), // Warm White
    (255, 255, 255), // Pure White
];

fn interpolate_color(t: f64) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let n = GRADIENT_STOPS.len() - 1;
    let idx = ((t * n as f64).floor() as usize).min(n - 1);
    let frac = (t * n as f64) - idx as f64;
    let c1 = GRADIENT_STOPS[idx];
    let c2 = GRADIENT_STOPS[idx + 1];
    (
        (c1.0 as f64 + (c2.0 as f64 - c1.0 as f64) * frac).round() as u8,
        (c1.1 as f64 + (c2.1 as f64 - c1.1 as f64) * frac).round() as u8,
        (c1.2 as f64 + (c2.2 as f64 - c1.2 as f64) * frac).round() as u8,
    )
}

/// Calculate visible string length ignoring ANSI escape codes.
///
/// Crucial for terminal grid alignment: terminal emulators advance the cursor
/// for printable characters but consume ANSI color codes invisibly. If you use
/// `.len()` or `.chars().count()` on an ANSI string, column borders will drift
/// and turn the box into a jagged mess.
pub fn visible_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_esc = false;
    for ch in s.chars() {
        if ch == '\x1b' {
            in_esc = true;
        } else if in_esc {
            if ch == 'm' {
                in_esc = false;
            }
        } else {
            len += 1;
        }
    }
    len
}

/// Print the stylized FIGlet ASCII banner with DigiiCampus red-white gradient.
pub fn print_banner() {
    let lines = [
        "  ██████╗ ██╗   ██╗  █████╗  ██████╗ ",
        " ██╔═══██╗██║   ██║ ██╔══██╗ ██╔══██╗",
        " ██║   ██║██║   ██║ ███████║ ██║  ██║",
        " ██║▄▄ ██║██║   ██║ ██╔══██║ ██║  ██║",
        " ╚██████╔╝╚██████╔╝ ██║  ██║ ██████╔╝",
        "  ╚══▀▀═╝  ╚═════╝  ╚═╝  ╚═╝ ╚═════╝ ",
    ];

    let color = is_color_terminal();
    let max_w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(36) as f64;
    let max_h = lines.len() as f64;

    println!();
    for (y, line) in lines.iter().enumerate() {
        let mut row = String::with_capacity(line.len() * 4);
        for (x, ch) in line.chars().enumerate() {
            if ch == ' ' || !color {
                row.push(ch);
            } else {
                let t = ((x as f64 / max_w) * 0.75 + (y as f64 / max_h) * 0.25).clamp(0.0, 1.0);
                let (r, g, b) = interpolate_color(t);
                row.push_str(&format!("\x1b[38;2;{r};{g};{b};1m{ch}\x1b[0m"));
            }
        }
        println!("{row}");
    }

    if color {
        println!(
            "\x1b[90m  ── \x1b[38;2;220;38;38;1m›\x1b[0m \x1b[37;1mDigiiCampus Student Intelligence CLI\x1b[0m \x1b[90m──\x1b[0m\n"
        );
    } else {
        println!("  ── › DigiiCampus Student Intelligence CLI ──\n");
    }
}

/// Rich Hermes-style two-column terminal dashboard.
///
/// Layout geometry:
/// ╭──────────────── quad v0.1.0 (DigiiCampus Student Intelligence) ────────────────╮
/// │ Left: ASCII Art + Student Identity   │ Right: Available Tools & Agent MCP     │
/// ├──────────────────────────────────────┴────────────────────────────────────────┤
/// │ Summary Footer: tools count · classes · timezone · diagnostics hint           │
/// ╰──────────────────────────────────────────────────────────────────────────────╯
pub fn print_dashboard(cfg: Option<&Config>) {
    let color = is_color_terminal();
    let left_w: usize = 36;
    let right_w: usize = 35;
    let inner_w: usize = left_w + right_w + 5;

    // Build Left Column (Hero ASCII art + Student session credentials)
    let mut left_col = Vec::new();
    let ascii_art = [
        " ██████╗ ██╗   ██╗  █████╗  ██████╗ ",
        "██╔═══██╗██║   ██║ ██╔══██╗ ██╔══██╗",
        "██║   ██║██║   ██║ ███████║ ██║  ██║",
        "██║▄▄ ██║██║   ██║ ██╔══██║ ██║  ██║",
        "╚██████╔╝╚██████╔╝ ██║  ██║ ██████╔╝",
        " ╚══▀▀═╝  ╚═════╝  ╚═╝  ╚═╝ ╚═════╝ ",
    ];

    for (y, line) in ascii_art.iter().enumerate() {
        if !color {
            left_col.push(line.to_string());
        } else {
            let mut colored = String::new();
            for (x, ch) in line.chars().enumerate() {
                if ch == ' ' {
                    colored.push(' ');
                } else {
                    let t = ((x as f64 / 36.0) * 0.75 + (y as f64 / 6.0) * 0.25).clamp(0.0, 1.0);
                    let (r, g, b) = interpolate_color(t);
                    colored.push_str(&format!("\x1b[38;2;{r};{g};{b};1m{ch}\x1b[0m"));
                }
            }
            left_col.push(colored);
        }
    }

    left_col.push(String::new());
    if color {
        left_col.push("\x1b[1mStudent Session:\x1b[0m".to_string());
    } else {
        left_col.push("Student Session:".to_string());
    }

    if let Some(c) = cfg {
        let claims = jwt_claims(&c.token).ok();
        let sub = claims
            .as_ref()
            .map(|m| clean(m.get("sub")))
            .unwrap_or_default();
        let college = c.college.as_deref().unwrap_or("Active Institute");
        let raw_portal = if c.base_url.is_empty() {
            "unconfigured"
        } else {
            c.base_url
                .trim_start_matches("https://")
                .trim_start_matches("http://")
        };
        let portal_display = if raw_portal.len() > 22 {
            &raw_portal[..22]
        } else {
            raw_portal
        };
        let sub_display = if sub.is_empty() {
            college
        } else if sub.len() > 20 {
            &sub[..20]
        } else {
            &sub
        };

        if color {
            left_col.push(format!(
                "  \x1b[38;2;211;47;47;1m›\x1b[0m User:     \x1b[37;1m{sub_display}\x1b[0m"
            ));
            left_col.push(format!(
                "  \x1b[38;2;211;47;47;1m›\x1b[0m College:  \x1b[90m{college}\x1b[0m"
            ));
            left_col.push(format!(
                "  \x1b[38;2;211;47;47;1m›\x1b[0m Portal:   \x1b[90m{portal_display}\x1b[0m"
            ));
            if c.is_expired() {
                left_col.push(
                    "  \x1b[38;2;211;47;47;1m›\x1b[0m Status:   \x1b[38;2;239;68;68;1m✖ Expired\x1b[0m"
                        .to_string(),
                );
            } else {
                let days = c.days_left().unwrap_or(0);
                left_col.push(format!(
                    "  \x1b[38;2;211;47;47;1m›\x1b[0m Status:   \x1b[38;2;34;197;94;1m✔ Valid\x1b[0m ({days}d)"
                ));
            }
            left_col.push(format!(
                "  \x1b[38;2;211;47;47;1m›\x1b[0m Timezone: \x1b[90m{}\x1b[0m",
                c.timezone
            ));
        } else {
            left_col.push(format!("  › User:     {sub_display}"));
            left_col.push(format!("  › College:  {college}"));
            left_col.push(format!("  › Portal:   {portal_display}"));
            if c.is_expired() {
                left_col.push("  › Status:   ✖ Expired".to_string());
            } else {
                let days = c.days_left().unwrap_or(0);
                left_col.push(format!("  › Status:   ✔ Valid ({days}d)"));
            }
            left_col.push(format!("  › Timezone: {}", c.timezone));
        }
    } else if color {
        left_col.push("  \x1b[90m• Status: Not logged in\x1b[0m".to_string());
        left_col.push(
            "  \x1b[38;2;220;38;38;1m›\x1b[0m Run \x1b[37;1mquad login\x1b[0m to authenticate"
                .to_string(),
        );
    } else {
        left_col.push("  • Status: Not logged in".to_string());
        left_col.push("  › Run `quad login` to authenticate".to_string());
    }

    // Build Right Column (Available Tools + Agents + Skills)
    let mut right_col = Vec::new();
    if color {
        right_col.push("\x1b[38;2;211;47;47;1mAvailable Tools\x1b[0m".to_string());
        right_col.push(
            "  \x1b[90mAcademics:\x1b[0m \x1b[37;1mdue\x1b[0m, \x1b[37;1mattendance\x1b[0m,"
                .to_string(),
        );
        right_col
            .push("             \x1b[37;1massignments\x1b[0m, \x1b[37;1mgrades\x1b[0m".to_string());
        right_col.push(
            "  \x1b[90mRecords:\x1b[0m   \x1b[37;1mclasses\x1b[0m, \x1b[37;1maccount\x1b[0m"
                .to_string(),
        );
        right_col
            .push("  \x1b[90mAgentic:\x1b[0m   \x1b[37;1msubmit_assignment\x1b[0m,".to_string());
        right_col
            .push("             \x1b[37;1msubmit_quiz\x1b[0m, \x1b[37;1mforms\x1b[0m".to_string());
        right_col.push(String::new());

        right_col.push("\x1b[38;2;211;47;47;1mConnected Agents & MCP\x1b[0m".to_string());
        let (ag1, ag2) = format_agent_status(color);
        right_col.push(ag1);
        right_col.push(ag2);
        right_col.push(String::new());

        right_col.push("\x1b[38;2;211;47;47;1mAvailable Skills\x1b[0m".to_string());
        right_col.push("  \x1b[37;1mquad:\x1b[0m deadlines, bunks, quiz".to_string());
    } else {
        right_col.push("Available Tools".to_string());
        right_col.push("  Academics: due, attendance,".to_string());
        right_col.push("             assignments, grades".to_string());
        right_col.push("  Records:   classes, account".to_string());
        right_col.push("  Agentic:   submit_assignment,".to_string());
        right_col.push("             submit_quiz, forms".to_string());
        right_col.push(String::new());

        right_col.push("Connected Agents & MCP".to_string());
        let (ag1, ag2) = format_agent_status(color);
        right_col.push(ag1);
        right_col.push(ag2);
        right_col.push(String::new());

        right_col.push("Available Skills".to_string());
        right_col.push("  quad: deadlines, bunks, quiz".to_string());
    }

    let max_rows = left_col.len().max(right_col.len());
    let border_color = if color { "\x1b[38;2;160;20;30;1m" } else { "" };
    let rst = if color { "\x1b[0m" } else { "" };

    let title = if color {
        format!(
            " \x1b[37;1mquad v{}\x1b[0m \x1b[90m(DigiiCampus Student Intelligence)\x1b[0m ",
            env!("CARGO_PKG_VERSION")
        )
    } else {
        format!(
            " quad v{} (DigiiCampus Student Intelligence) ",
            env!("CARGO_PKG_VERSION")
        )
    };

    let title_vis = visible_len(&title);
    let dashes = inner_w.saturating_sub(title_vis);
    let ld = dashes / 2;
    let rd = dashes.saturating_sub(ld);

    println!();
    println!(
        "{border_color}╭{ld_str}{rst}{title}{border_color}{rd_str}╮{rst}",
        ld_str = "─".repeat(ld),
        rd_str = "─".repeat(rd)
    );

    for r in 0..max_rows {
        let l = left_col.get(r).map(String::as_str).unwrap_or("");
        let rg = right_col.get(r).map(String::as_str).unwrap_or("");
        let l_pad = left_w.saturating_sub(visible_len(l));
        let r_pad = right_w.saturating_sub(visible_len(rg));
        println!(
            "{border_color}│{rst} {l}{l_sp} {border_color}│{rst} {rg}{r_sp} {border_color}│{rst}",
            l_sp = " ".repeat(l_pad),
            r_sp = " ".repeat(r_pad)
        );
    }

    let status = "14 tools · Asia/Kolkata · quad doctor · /help";
    let s_pad = inner_w.saturating_sub(visible_len(status) + 2);

    let mid = format!(
        "{border_color}├{l_div}┴{r_div}┤{rst}",
        l_div = "─".repeat(left_w + 2),
        r_div = "─".repeat(right_w + 2)
    );
    let bot = format!("{border_color}╰{b_div}╯{rst}", b_div = "─".repeat(inner_w));

    println!("{mid}");
    if color {
        println!(
            "{border_color}│{rst} \x1b[90m{status}\x1b[0m{sp} {border_color}│{rst}",
            sp = " ".repeat(s_pad)
        );
    } else {
        println!(
            "{border_color}│{rst} {status}{sp} {border_color}│{rst}",
            sp = " ".repeat(s_pad)
        );
    }
    println!("{bot}\n");
}

fn format_agent_status(color: bool) -> (String, String) {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let (claude, codex, cursor, gemini) = if let Some(h) = home {
        (
            h.join(".claude").exists(),
            h.join(".codex").exists(),
            h.join(".cursor").exists(),
            h.join(".gemini").exists(),
        )
    } else {
        (false, false, false, false)
    };

    let tag = |name: &str, ready: bool| {
        if color {
            if ready {
                format!("{name} (\x1b[38;2;34;197;94mready\x1b[0m)")
            } else {
                format!("{name} (\x1b[90munconfigured\x1b[0m)")
            }
        } else {
            if ready {
                format!("{name} (ready)")
            } else {
                format!("{name} (unconfigured)")
            }
        }
    };

    (
        format!("  {}  {}", tag("Claude", claude), tag("Codex", codex)),
        format!("  {}  {}", tag("Cursor", cursor), tag("Gemini", gemini)),
    )
}

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Interactive animatable terminal spinner for long-running CLI operations.
pub struct Spinner {
    msg: String,
    frame: usize,
    start: Instant,
    is_tty: bool,
}

impl Spinner {
    pub fn new(msg: impl Into<String>) -> Self {
        let is_tty = is_color_terminal();
        let msg = msg.into();
        let s = Self {
            msg,
            frame: 0,
            start: Instant::now(),
            is_tty,
        };
        if !is_tty {
            println!("  › {}", s.msg);
        }
        s
    }

    pub fn tick(&mut self) {
        if !self.is_tty {
            return;
        }
        let frame = SPINNER_FRAMES[self.frame % SPINNER_FRAMES.len()];
        self.frame += 1;
        let elapsed = self.start.elapsed().as_secs_f32();
        print!(
            "\r  \x1b[38;2;220;38;38;1m{frame}\x1b[0m {} \x1b[90m({:.1}s)\x1b[0m\x1b[K",
            self.msg, elapsed
        );
        let _ = stdout().flush();
    }

    pub fn finish_ok(&mut self, final_msg: &str) {
        if self.is_tty {
            println!("\r  \x1b[38;2;34;197;94;1m✔\x1b[0m {final_msg}\x1b[K");
        } else {
            println!("  ✔ {final_msg}");
        }
    }

    pub fn finish_err(&mut self, final_msg: &str) {
        if self.is_tty {
            println!("\r  \x1b[38;2;239;68;68;1m✖\x1b[0m {final_msg}\x1b[K");
        } else {
            println!("  ✖ {final_msg}");
        }
    }
}

pub fn ok(msg: &str) {
    if is_color_terminal() {
        println!("  \x1b[38;2;34;197;94;1m✔\x1b[0m {msg}");
    } else {
        println!("  ✔ {msg}");
    }
}

pub fn err(msg: &str) {
    if is_color_terminal() {
        println!("  \x1b[38;2;239;68;68;1m✖\x1b[0m {msg}");
    } else {
        println!("  ✖ {msg}");
    }
}

pub fn step(msg: &str) {
    if is_color_terminal() {
        println!("  \x1b[38;2;220;38;38;1m›\x1b[0m {msg}");
    } else {
        println!("  › {msg}");
    }
}

pub fn info(msg: &str) {
    if is_color_terminal() {
        println!("  \x1b[90m•\x1b[0m {msg}");
    } else {
        println!("  • {msg}");
    }
}
