//! TUI presentation: DigiiCampus red-to-white gradient FIGlet banner,
//! animatable braille spinners, and clean status badges without emojis.

use std::io::{IsTerminal, Write, stdout};
use std::time::Instant;

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
