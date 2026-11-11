//! TUI presentation: multi-theme support (Pure Black, Tokyo Night / Midnight,
//! Crimson Dark / Digii, and Cyberpunk / Matrix Emerald), animatable braille
//! spinners, clean status badges, and a rich Hermes-style two-column terminal dashboard.
//!
//! Human engineering rationale:
//! CLI tools are often either plain and boring or overloaded with tacky emojis
//! that render with uneven widths across different terminal emulators (macOS
//! Terminal vs iTerm2 vs Alacritty vs Ghostty vs Windows Terminal).
//!
//! Here we adopt a clean, authoritative TUI style:
//!   * Selectable distinct themes without sloppy blue accents:
//!       - Pure Black (pitch black #000000, crisp neutral silver & bright white)
//!       - Tokyo Night / Midnight (deep indigo #1a1b26, purple, cyan, coral)
//!       - Crimson Dark / Digii (deep obsidian crimson #0f0507, Digii brand red)
//!       - Cyberpunk / Matrix Emerald (matrix black #050d08, phosphor emerald & mint)
//!   * Crisp Unicode box-drawing glyphs (`╭─╮`, `│`, `╰─╯`)
//!   * Uniform-width status markers (`✔`, `✖`, `›`, `•`)
//!   * A two-column dashboard (inspired by Hermes CLI) displaying student session
//!     identity on the left and agent status / capabilities on the right.

use std::io::{IsTerminal, Write, stdout};
use std::time::Instant;

use crate::config::Config;
use crate::error::jwt_claims;
use crate::portal::clean;

/// Available visual themes for the terminal dashboard and UI elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    PureBlack,
    TokyoNight,
    CrimsonDark,
    Cyberpunk,
}

impl Theme {
    pub const ALL: &'static [Theme] = &[
        Theme::PureBlack,
        Theme::TokyoNight,
        Theme::CrimsonDark,
        Theme::Cyberpunk,
    ];

    /// Parse theme from string with loose matching.
    pub fn from_str_loose(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase().replace(['_', ' '], "-");
        match s.as_str() {
            "pure-black" | "pureblack" | "black" | "pure" | "mono" | "monochrome" => {
                Some(Theme::PureBlack)
            }
            "tokyo-night" | "tokyonight" | "tokyo" | "midnight" | "night" => {
                Some(Theme::TokyoNight)
            }
            "crimson-dark" | "crimsondark" | "crimson" | "digii" | "digimcp" | "red" => {
                Some(Theme::CrimsonDark)
            }
            "cyberpunk" | "matrix" | "matrix-emerald" | "matrixemerald" | "emerald" | "green" => {
                Some(Theme::Cyberpunk)
            }
            _ => None,
        }
    }

    /// Resolve the current active theme from environment or config.
    pub fn current() -> Self {
        if let Some(val) = crate::config::lookup("DIGIMCP_THEME")
            .or_else(|| crate::config::lookup("DIGI_THEME"))
            .or_else(|| crate::config::lookup("THEME"))
            && let Some(t) = Self::from_str_loose(&val)
        {
            return t;
        }
        Theme::PureBlack
    }

    pub fn name(&self) -> &'static str {
        match self {
            Theme::PureBlack => "pure-black",
            Theme::TokyoNight => "tokyo-night",
            Theme::CrimsonDark => "crimson-dark",
            Theme::Cyberpunk => "matrix-emerald",
        }
    }

    /// Parse `--theme <val>` or `--theme=<val>` arguments, persisting into `DIGIMCP_THEME`.
    pub fn apply_args(args: &[String]) -> Self {
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            let val = if arg == "--theme" {
                iter.next().map(String::as_str)
            } else {
                arg.strip_prefix("--theme=")
            };
            if let Some(raw) = val
                && let Some(theme) = Self::from_str_loose(raw)
            {
                // SAFETY: early synchronous CLI dispatch before spawning an async runtime.
                unsafe { std::env::set_var("DIGIMCP_THEME", theme.name()) };
                return theme;
            }
        }
        Self::current()
    }

    pub fn border_rgb(&self) -> (u8, u8, u8) {
        (100, 116, 139) // Slate 500 hairline across all themes
    }

    pub fn accent_rgb(&self) -> (u8, u8, u8) {
        match self {
            Theme::PureBlack => (248, 250, 252),  // Crisp Slate 50 White
            Theme::TokyoNight => (187, 154, 247), // #bb9af7 Purple
            Theme::CrimsonDark => (211, 47, 47),  // Digii Red
            Theme::Cyberpunk => (52, 211, 153),   // Bright Mint
        }
    }

    pub fn success_rgb(&self) -> (u8, u8, u8) {
        match self {
            Theme::PureBlack => (34, 197, 94),    // Green
            Theme::TokyoNight => (158, 206, 106), // #9ece6a Tokyo Night green
            Theme::CrimsonDark => (34, 197, 94),  // Green
            Theme::Cyberpunk => (74, 222, 128),   // Phosphor Lime
        }
    }

    pub fn error_rgb(&self) -> (u8, u8, u8) {
        match self {
            Theme::PureBlack => (239, 68, 68),
            Theme::TokyoNight => (247, 118, 142),
            Theme::CrimsonDark => (239, 68, 68),
            Theme::Cyberpunk => (248, 113, 113),
        }
    }

    pub fn muted_rgb(&self) -> (u8, u8, u8) {
        match self {
            Theme::PureBlack => (148, 163, 184),   // Slate 400
            Theme::TokyoNight => (122, 162, 247),  // #7aa2f7 Soft Blue
            Theme::CrimsonDark => (156, 163, 175), // Gray 400
            Theme::Cyberpunk => (75, 140, 110),    // Muted Emerald
        }
    }

    fn fg_ansi(rgb: (u8, u8, u8), bold: bool) -> String {
        if bold {
            format!("\x1b[38;2;{};{};{};1m", rgb.0, rgb.1, rgb.2)
        } else {
            format!("\x1b[38;2;{};{};{}m", rgb.0, rgb.1, rgb.2)
        }
    }

    pub fn border_ansi(&self) -> String {
        Self::fg_ansi(self.border_rgb(), false)
    }

    pub fn accent_ansi(&self) -> String {
        Self::fg_ansi(self.accent_rgb(), true)
    }

    pub fn header_ansi(&self) -> String {
        self.accent_ansi()
    }

    pub fn success_ansi(&self) -> String {
        Self::fg_ansi(self.success_rgb(), true)
    }

    pub fn error_ansi(&self) -> String {
        Self::fg_ansi(self.error_rgb(), true)
    }

    pub fn muted_ansi(&self) -> String {
        Self::fg_ansi(self.muted_rgb(), false)
    }
}

/// Check whether stdout is attached to an interactive terminal supporting ANSI color.
pub fn is_color_terminal() -> bool {
    (stdout().is_terminal()
        || std::env::var_os("FORCE_COLOR").is_some()
        || std::env::var_os("CLICOLOR_FORCE").is_some())
        && std::env::var_os("NO_COLOR").is_none()
}

/// Calculate visible string length ignoring ANSI escape codes.
pub fn visible_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_esc = false;
    for ch in s.chars() {
        if ch == '\x1b' {
            in_esc = true;
        } else if in_esc {
            if ch.is_ascii_alphabetic() {
                in_esc = false;
            }
        } else {
            len += 1;
        }
    }
    len
}

/// Print the stylized FIGlet ASCII banner with the current theme gradient.
pub fn print_banner() {
    print_banner_themed(Theme::current());
}

/// Print the stylized FIGlet ASCII banner with a specific visual theme.
pub fn print_banner_themed(_theme: Theme) {
    let lines = [
        "  ██████╗ ██╗ ██████╗ ██╗███╗   ███╗ ██████╗██████╗ ",
        "  ██╔══██╗██║██╔════╝ ██║████╗ ████║██╔════╝██╔══██╗",
        "  ██║  ██║██║██║  ███╗██║██╔████╔██║██║     ██████╔╝",
        "  ██║  ██║██║██║   ██║██║██║╚██╔╝██║██║     ██╔═══╝ ",
        "  ██████╔╝██║╚██████╔╝██║██║ ╚═╝ ██║╚██████╗██║     ",
        "  ╚═════╝ ╚═╝ ╚═════╝ ╚═╝╚═╝     ╚═╝ ╚═════╝╚═╝     ",
    ];

    let color = is_color_terminal();
    println!();
    for line in &lines {
        if color {
            println!("\x1b[38;2;226;232;240m{line}\x1b[0m");
        } else {
            println!("{line}");
        }
    }

    if color {
        println!(
            "\x1b[38;2;148;163;184m  ── \x1b[38;2;248;250;252m›\x1b[0m \x1b[37;1mDigiiCampus Student Intelligence CLI\x1b[0m \x1b[38;2;148;163;184m──\x1b[0m\n"
        );
    } else {
        println!("  ── › DigiiCampus Student Intelligence CLI ──\n");
    }
}

/// Truncate string to at most `max_chars` Unicode characters safely along char boundaries.
pub fn truncate_str(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Rich Hermes-style two-column terminal dashboard (current active theme).
pub fn print_dashboard(cfg: Option<&Config>) {
    print_dashboard_themed(cfg, Theme::current());
}

const DIGI_MARK: [&str; 6] = [
    "██████╗ ██╗ ██████╗ ██╗",
    "██╔══██╗██║██╔════╝ ██║",
    "██║  ██║██║██║  ███╗██║",
    "██║  ██║██║██║   ██║██║",
    "██████╔╝██║╚██████╔╝██║",
    "╚═════╝ ╚═╝ ╚═════╝ ╚═╝",
];

fn style_hdr(title: &str, color: bool, header_ansi: &str) -> String {
    if color {
        format!("{header_ansi}{title}\x1b[0m")
    } else {
        title.to_string()
    }
}

fn style_kv(label: &str, val: &str, val_ansi: &str, muted_ansi: &str, color: bool) -> String {
    if color {
        format!("{muted_ansi}{label:<8}\x1b[0m{val_ansi}{val}\x1b[0m")
    } else {
        format!("{label:<8}{val}")
    }
}

fn format_session_status(c: &Config) -> String {
    if c.is_expired() {
        let days = (-c.days_left().unwrap_or(0)).max(0);
        if days == 0 {
            "✖ Expired today".to_string()
        } else if days <= 99 {
            format!("✖ Expired ({days}d)")
        } else {
            "✖ Exp (>99d)".to_string()
        }
    } else {
        match c.days_left() {
            None => "✔ Valid".to_string(),
            Some(days) if days <= 0 => "✔ Valid (<24h)".to_string(),
            Some(days) if days <= 999 => format!("✔ Valid ({days}d)"),
            Some(_) => "✔ Valid (>999d)".to_string(),
        }
    }
}

/// Pure function returning every rendered line of the two-column dashboard box using explicit color control.
pub fn render_dashboard_lines_colored(
    cfg: Option<&Config>,
    theme: Theme,
    color: bool,
) -> Vec<String> {
    let left_w: usize = 23;
    let right_w: usize = 48;
    let l_div = left_w + 2;
    let r_div = right_w + 2;
    let inner_w: usize = l_div + 1 + r_div;

    let border_ansi = if color {
        theme.border_ansi()
    } else {
        String::new()
    };
    let accent_ansi = if color {
        theme.accent_ansi()
    } else {
        String::new()
    };
    let success_ansi = if color {
        theme.success_ansi()
    } else {
        String::new()
    };
    let error_ansi = if color {
        theme.error_ansi()
    } else {
        String::new()
    };
    let muted_ansi = if color {
        theme.muted_ansi()
    } else {
        String::new()
    };
    let rst = if color { "\x1b[0m" } else { "" };

    // Left column: compact flat-accent DIGI mark + Student session (13 rows total, zero dead space).
    let mut left_col = Vec::with_capacity(13);
    for row in DIGI_MARK {
        if color {
            left_col.push(format!("{accent_ansi}{row}\x1b[0m"));
        } else {
            left_col.push(row.to_string());
        }
    }
    left_col.push(String::new());
    left_col.push(style_hdr("Student session", color, &accent_ansi));

    if let Some(c) = cfg {
        let claims = jwt_claims(&c.token).ok();
        let sub = claims
            .as_ref()
            .map(|m| clean(m.get("sub")))
            .unwrap_or_default();
        let raw_college = c.college.as_deref().unwrap_or("Active Institute");
        let college_display = truncate_str(raw_college, 15);
        let raw_portal = if c.base_url.is_empty() {
            "unconfigured"
        } else {
            c.base_url
                .trim_start_matches("https://")
                .trim_start_matches("http://")
        };
        let portal_display = truncate_str(raw_portal, 15);
        let sub_display = if sub.is_empty() {
            college_display
        } else {
            truncate_str(&sub, 15)
        };
        let tz_display = truncate_str(&c.timezone, 15);
        let status_str = format_session_status(c);
        let status_ansi = if c.is_expired() {
            &error_ansi
        } else {
            &success_ansi
        };

        left_col.push(style_kv(
            "User",
            sub_display,
            "\x1b[37;1m",
            &muted_ansi,
            color,
        ));
        left_col.push(style_kv("College", college_display, "", &muted_ansi, color));
        left_col.push(style_kv(
            "Portal",
            portal_display,
            &muted_ansi,
            &muted_ansi,
            color,
        ));
        left_col.push(style_kv(
            "Status",
            &status_str,
            status_ansi,
            &muted_ansi,
            color,
        ));
        left_col.push(style_kv(
            "Zone",
            tz_display,
            &muted_ansi,
            &muted_ansi,
            color,
        ));
    } else {
        left_col.push(style_kv(
            "Status",
            "Not logged in",
            &error_ansi,
            &muted_ansi,
            color,
        ));
        left_col.push(style_kv(
            "Login",
            "digi login",
            "\x1b[37;1m",
            &muted_ansi,
            color,
        ));
        left_col.push(style_kv(
            "Manual",
            "--token <jwt>",
            &muted_ansi,
            &muted_ansi,
            color,
        ));
    }

    // Right column: strictly single-line tool categories + 2-line agents + commands & timezone footer (13 rows).
    let tz_footer = truncate_str(cfg.map_or("Asia/Kolkata", |c| c.timezone.as_str()), 21);
    let mut right_col = Vec::with_capacity(13);
    right_col.push(style_hdr("Available tools (16)", color, &accent_ansi));
    let categories = [
        ("Academics", "due, attendance, assignments, grades"),
        ("Materials", "materials, material"),
        ("Records", "classes, account, search"),
        ("Agentic", "submit_assignment, submit_quiz, forms"),
    ];
    for (cat, tools) in categories {
        if color {
            right_col.push(format!(
                " {muted_ansi}{cat:<10}\x1b[0m\x1b[37m{tools}\x1b[0m"
            ));
        } else {
            right_col.push(format!(" {cat:<10}{tools}"));
        }
    }
    right_col.push(String::new());

    right_col.push(style_hdr("Connected agents", color, &accent_ansi));
    let (ag1, ag2) = format_agent_status(color, theme);
    right_col.push(ag1);
    right_col.push(ag2);
    right_col.push(String::new());

    right_col.push(style_hdr("Commands", color, &accent_ansi));
    let cmd_row = " digi login · digi setup · digi doctor";
    let footer_row = format!(" 16 tools · {tz_footer} · digi --help");
    if color {
        right_col.push(format!("\x1b[37m{cmd_row}\x1b[0m"));
        right_col.push(format!("{muted_ansi}{footer_row}\x1b[0m"));
    } else {
        right_col.push(cmd_row.to_string());
        right_col.push(footer_row);
    }

    let max_rows = left_col.len().max(right_col.len());
    let title = if color {
        format!(
            " \x1b[37;1mDigiMCP v{}\x1b[0m {muted_ansi}· DigiiCampus MCP Server\x1b[0m ",
            env!("CARGO_PKG_VERSION")
        )
    } else {
        format!(
            " DigiMCP v{} · DigiiCampus MCP Server ",
            env!("CARGO_PKG_VERSION")
        )
    };

    let title_vis = visible_len(&title);
    let ld = 1;
    let rd = inner_w.saturating_sub(title_vis + ld);

    let mut lines = Vec::with_capacity(max_rows + 2);
    lines.push(format!(
        "{border_ansi}╭{ld_str}{rst}{title}{border_ansi}{rd_str}╮{rst}",
        ld_str = "─".repeat(ld),
        rd_str = "─".repeat(rd)
    ));

    for r in 0..max_rows {
        let l = left_col.get(r).map(String::as_str).unwrap_or("");
        let rg = right_col.get(r).map(String::as_str).unwrap_or("");
        let l_pad = left_w.saturating_sub(visible_len(l));
        let r_pad = right_w.saturating_sub(visible_len(rg));
        lines.push(format!(
            "{border_ansi}│{rst} {l}{l_sp} {border_ansi}│{rst} {rg}{r_sp} {border_ansi}│{rst}",
            l_sp = " ".repeat(l_pad),
            r_sp = " ".repeat(r_pad)
        ));
    }

    lines.push(format!(
        "{border_ansi}╰{l_div_str}┴{r_div_str}╯{rst}",
        l_div_str = "─".repeat(l_div),
        r_div_str = "─".repeat(r_div)
    ));

    lines
}

/// Pure function returning every rendered line of the two-column dashboard box using terminal color detection.
pub fn render_dashboard_lines(cfg: Option<&Config>, theme: Theme) -> Vec<String> {
    render_dashboard_lines_colored(cfg, theme, is_color_terminal())
}

/// Rich Hermes-style two-column terminal dashboard rendered in a specific visual theme.
pub fn print_dashboard_themed(cfg: Option<&Config>, theme: Theme) {
    println!();
    for line in render_dashboard_lines(cfg, theme) {
        println!("{line}");
    }
    println!();
}

fn format_agent_status(color: bool, theme: Theme) -> (String, String) {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let (claude, codex, cursor, gemini, opencode) = if let Some(h) = home {
        (
            h.join(".claude").exists(),
            h.join(".codex").exists(),
            h.join(".cursor").exists(),
            h.join(".gemini").exists(),
            h.join(".config").join("opencode").exists(),
        )
    } else {
        (false, false, false, false, false)
    };

    let tag = |name: &str, ready: bool| {
        let state = if ready { "ready" } else { "unset" };
        if color {
            let code = if ready {
                theme.success_ansi()
            } else {
                theme.muted_ansi()
            };
            format!("{name} ({code}{state}\x1b[0m)")
        } else {
            format!("{name} ({state})")
        }
    };

    let sep = if color {
        format!(" {}·\x1b[0m ", theme.muted_ansi())
    } else {
        " · ".to_string()
    };

    (
        format!(
            " {}{sep}{}{sep}{}",
            tag("Claude", claude),
            tag("Codex", codex),
            tag("Cursor", cursor)
        ),
        format!(
            " {}{sep}{}",
            tag("Gemini", gemini),
            tag("OpenCode", opencode)
        ),
    )
}

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Interactive animatable terminal spinner for long-running CLI operations.
pub struct Spinner {
    msg: String,
    frame: usize,
    start: Instant,
    is_tty: bool,
    accent_ansi: String,
    muted_ansi: String,
    success_ansi: String,
    error_ansi: String,
}

impl Spinner {
    pub fn new(msg: impl Into<String>) -> Self {
        let is_tty = is_color_terminal();
        let msg = msg.into();
        let theme = Theme::current();
        let s = Self {
            msg,
            frame: 0,
            start: Instant::now(),
            is_tty,
            accent_ansi: theme.accent_ansi(),
            muted_ansi: theme.muted_ansi(),
            success_ansi: theme.success_ansi(),
            error_ansi: theme.error_ansi(),
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
        let tenths = self.start.elapsed().as_millis() / 100;
        print!(
            "\r  {}{frame}\x1b[0m {} {}({}.{}s)\x1b[0m\x1b[K",
            self.accent_ansi,
            self.msg,
            self.muted_ansi,
            tenths / 10,
            tenths % 10
        );
        let _ = stdout().flush();
    }

    pub fn finish_ok(&mut self, final_msg: &str) {
        if self.is_tty {
            println!("\r  {}✔\x1b[0m {final_msg}\x1b[K", self.success_ansi);
        } else {
            println!("  ✔ {final_msg}");
        }
    }

    pub fn finish_err(&mut self, final_msg: &str) {
        if self.is_tty {
            println!("\r  {}✖\x1b[0m {final_msg}\x1b[K", self.error_ansi);
        } else {
            println!("  ✖ {final_msg}");
        }
    }
}

pub fn ok(msg: &str) {
    if is_color_terminal() {
        let code = Theme::current().success_ansi();
        println!("  {code}✔\x1b[0m {msg}");
    } else {
        println!("  ✔ {msg}");
    }
}

pub fn err(msg: &str) {
    if is_color_terminal() {
        let code = Theme::current().error_ansi();
        println!("  {code}✖\x1b[0m {msg}");
    } else {
        println!("  ✖ {msg}");
    }
}

pub fn step(msg: &str) {
    if is_color_terminal() {
        let code = Theme::current().accent_ansi();
        println!("  {code}›\x1b[0m {msg}");
    } else {
        println!("  › {msg}");
    }
}

pub fn info(msg: &str) {
    if is_color_terminal() {
        let code = Theme::current().muted_ansi();
        println!("  {code}•\x1b[0m {msg}");
    } else {
        println!("  • {msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_from_str_loose() {
        assert_eq!(Theme::from_str_loose("pure-black"), Some(Theme::PureBlack));
        assert_eq!(Theme::from_str_loose("Pure_Black"), Some(Theme::PureBlack));
        assert_eq!(Theme::from_str_loose("pure black"), Some(Theme::PureBlack));
        assert_eq!(Theme::from_str_loose("black"), Some(Theme::PureBlack));
        assert_eq!(
            Theme::from_str_loose("tokyo-night"),
            Some(Theme::TokyoNight)
        );
        assert_eq!(
            Theme::from_str_loose("tokyo night"),
            Some(Theme::TokyoNight)
        );
        assert_eq!(Theme::from_str_loose("tokyo"), Some(Theme::TokyoNight));
        assert_eq!(Theme::from_str_loose("midnight"), Some(Theme::TokyoNight));
        assert_eq!(
            Theme::from_str_loose("crimson-dark"),
            Some(Theme::CrimsonDark)
        );
        assert_eq!(
            Theme::from_str_loose("crimson dark"),
            Some(Theme::CrimsonDark)
        );
        assert_eq!(Theme::from_str_loose("digii"), Some(Theme::CrimsonDark));
        assert_eq!(Theme::from_str_loose("crimson"), Some(Theme::CrimsonDark));
        assert_eq!(Theme::from_str_loose("cyberpunk"), Some(Theme::Cyberpunk));
        assert_eq!(Theme::from_str_loose("matrix"), Some(Theme::Cyberpunk));
        assert_eq!(
            Theme::from_str_loose("matrix-emerald"),
            Some(Theme::Cyberpunk)
        );
        assert_eq!(
            Theme::from_str_loose("matrix emerald"),
            Some(Theme::Cyberpunk)
        );
        assert_eq!(
            Theme::from_str_loose("matrixemerald"),
            Some(Theme::Cyberpunk)
        );
        assert_eq!(Theme::from_str_loose("pureblack"), Some(Theme::PureBlack));
        assert_eq!(Theme::from_str_loose("tokyonight"), Some(Theme::TokyoNight));
        assert_eq!(
            Theme::from_str_loose("crimsondark"),
            Some(Theme::CrimsonDark)
        );
        assert_eq!(Theme::from_str_loose("invalid-theme"), None);
    }

    #[test]
    fn test_truncate_str_unicode() {
        assert_eq!(truncate_str("", 19), "");
        assert_eq!(truncate_str("short", 19), "short");
        assert_eq!(truncate_str("short", 0), "");
        assert_eq!(
            truncate_str("abcdefghijklmnopqrs", 19),
            "abcdefghijklmnopqrs"
        );
        assert_eq!(
            truncate_str("abcdefghijklmnopqrstuvwxyz", 19),
            "abcdefghijklmnopqrs"
        );
        // Multibyte UTF-8 characters crossing boundary (2-byte umlauts)
        let unicode = "abcdefghijklmnopqrä"; // 19 chars, 20 bytes
        assert_eq!(truncate_str(unicode, 19), "abcdefghijklmnopqrä");
        let unicode_long = "abcdefghijklmnopqräextra"; // 24 chars
        assert_eq!(truncate_str(unicode_long, 19), "abcdefghijklmnopqrä");
        // 3-byte Devanagari characters
        let devanagari = "नमस्तेविद्यालयानन्तः";
        assert_eq!(truncate_str(devanagari, 6), "नमस्ते");
        // 4-byte emoji characters crossing boundary
        let emojis = "🦀⚡🎯🚀🔥💻📚🎓✨💡🌟🎉🍎🍕🐶🐱🦄🌈⚡✨";
        assert_eq!(truncate_str(emojis, 5), "🦀⚡🎯🚀🔥");
    }

    #[test]
    fn test_theme_palette_tokens() {
        for theme in Theme::ALL {
            assert!(theme.accent_ansi().starts_with("\x1b[38;2;"));
            assert!(theme.muted_ansi().starts_with("\x1b[38;2;"));
            assert_eq!(theme.header_ansi(), theme.accent_ansi());
        }
    }

    #[test]
    fn test_visible_len() {
        assert_eq!(visible_len("Hello"), 5);
        assert_eq!(visible_len("\x1b[31;1mHello\x1b[0m"), 5);
        assert_eq!(visible_len("\x1b[38;2;255;0;0;1mTest\x1b[0m!"), 5);
        assert_eq!(visible_len("\x1b[KHello"), 5);
        assert_eq!(visible_len("\x1b[2JClear"), 5);
    }

    #[test]
    fn test_border_styling() {
        for theme in Theme::ALL {
            assert_eq!(theme.border_rgb(), (100, 116, 139));
            assert_eq!(theme.border_ansi(), "\x1b[38;2;100;116;139m");
        }
    }

    fn make_test_jwt(sub: &str) -> String {
        let claims = format!(r#"{{"sub":"{sub}"}}"#);
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let bytes = claims.as_bytes();
        let mut encoded = String::new();
        for chunk in bytes.chunks(3) {
            let b0 = chunk[0];
            let b1 = chunk.get(1).copied().unwrap_or(0);
            let b2 = chunk.get(2).copied().unwrap_or(0);
            encoded.push(T[(b0 >> 2) as usize] as char);
            encoded.push(T[(((b0 & 3) << 4) | (b1 >> 4)) as usize] as char);
            if chunk.len() > 1 {
                encoded.push(T[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
            }
            if chunk.len() > 2 {
                encoded.push(T[(b2 & 0x3f) as usize] as char);
            }
        }
        format!("eyJhbGciOiJIUzI1NiJ9.{encoded}.sig")
    }

    fn make_test_config(
        sub: &str,
        college: Option<&str>,
        base_url: &str,
        timezone: &str,
        days_from_now: i64,
        expired: bool,
    ) -> Config {
        let now = crate::config::now_unix();
        let expires_at = if expired {
            if days_from_now <= 0 {
                now - 120
            } else {
                now - days_from_now * 86_400 - 43_200
            }
        } else {
            now + days_from_now * 86_400 + 43_200
        };

        Config {
            token: make_test_jwt(sub),
            base_url: base_url.to_string(),
            timezone: timezone.to_string(),
            timeout_ms: 40_000,
            ukid: Some(101),
            expires_at: Some(expires_at),
            college: college.map(String::from),
        }
    }

    #[test]
    fn test_dashboard_dimensions_within_78_columns() {
        let now = crate::config::now_unix();

        // 1. Matrix of token validity days required by specification
        let test_day_counts = [0, 1, 6, 9, 10, 14, 30, 90, 99, 180, 365, 1000];
        for days in test_day_counts {
            let cfg = make_test_config(
                "student1",
                Some("SNU Chennai"),
                "https://snuc.digiicampus.com",
                "Asia/Kolkata",
                days,
                false,
            );
            for theme in Theme::ALL {
                for color in [false, true] {
                    let lines = render_dashboard_lines_colored(Some(&cfg), *theme, color);
                    assert!(
                        !lines.is_empty(),
                        "Dashboard rendered no lines for days={days}"
                    );
                    for (idx, line) in lines.iter().enumerate() {
                        let vis = visible_len(line);
                        assert_eq!(
                            vis, 78,
                            "Line {idx} width ({vis} != 78) for valid token days={days}, theme={:?}, color={color}:\n{line}",
                            theme
                        );
                    }
                }
            }
        }

        // 2. Comprehensive regression sweep: Every single 2-digit day from 10 to 99
        for days in 10..=99 {
            let cfg = make_test_config(
                "user_2digit",
                Some("Active College"),
                "https://portal.edu",
                "Asia/Kolkata",
                days,
                false,
            );
            for theme in Theme::ALL {
                for color in [false, true] {
                    let lines = render_dashboard_lines_colored(Some(&cfg), *theme, color);
                    for (idx, line) in lines.iter().enumerate() {
                        let vis = visible_len(line);
                        assert_eq!(
                            vis, 78,
                            "Regression failure on 2-digit day count {days} at line {idx} (width {vis} != 78):\n{line}"
                        );
                    }
                }
            }
        }

        // 3. Expired token states (0 days, 1 day ago, 14 days ago, 30 days ago, 90 days ago, 180 days ago, 1000 days ago)
        for expired_days in [0, 1, 14, 30, 90, 180, 1000] {
            let cfg = make_test_config(
                "user_exp",
                Some("Expired Institute"),
                "https://exp.digii.com",
                "Asia/Kolkata",
                expired_days,
                true,
            );
            for theme in Theme::ALL {
                for color in [false, true] {
                    let lines = render_dashboard_lines_colored(Some(&cfg), *theme, color);
                    for (idx, line) in lines.iter().enumerate() {
                        let vis = visible_len(line);
                        assert_eq!(
                            vis, 78,
                            "Expired token ({expired_days}d ago) produced line {idx} with width {vis} != 78 (theme={:?}, color={color}):\n{line}",
                            theme
                        );
                    }
                }
            }
        }

        // 4. Unconfigured / Not logged in state (cfg = None)
        for theme in Theme::ALL {
            for color in [false, true] {
                let lines = render_dashboard_lines_colored(None, *theme, color);
                for (idx, line) in lines.iter().enumerate() {
                    let vis = visible_len(line);
                    assert_eq!(
                        vis, 78,
                        "Unconfigured dashboard line {idx} has width {vis} != 78 (theme={:?}, color={color}):\n{line}",
                        theme
                    );
                }
            }
        }

        // 5. Config with no expires_at timestamp
        let mut no_exp_cfg = make_test_config(
            "student_no_exp",
            Some("Active Institute"),
            "https://noexp.digii.com",
            "Asia/Kolkata",
            0,
            false,
        );
        no_exp_cfg.expires_at = None;
        for theme in Theme::ALL {
            for color in [false, true] {
                let lines = render_dashboard_lines_colored(Some(&no_exp_cfg), *theme, color);
                for (idx, line) in lines.iter().enumerate() {
                    let vis = visible_len(line);
                    assert_eq!(
                        vis, 78,
                        "No-expiry dashboard line {idx} has width {vis} != 78 (theme={:?}, color={color}):\n{line}",
                        theme
                    );
                }
            }
        }

        // 6. Extreme boundary conditions: Long username, long campus name, long portal, long timezone
        let boundary_cfg = Config {
            token: make_test_jwt(
                "super_extremely_long_username_that_should_be_truncated@domain.edu",
            ),
            base_url:
                "https://an-extraordinarily-long-subdomain-name-for-a-campus-portal.university.edu"
                    .to_string(),
            timezone: "America/Argentina/ComodRivadavia".to_string(),
            timeout_ms: 40_000,
            ukid: Some(999999),
            expires_at: Some(now + 14 * 86_400),
            college: Some(
                "Super Massive International University of Science and Engineering Campus"
                    .to_string(),
            ),
        };
        for theme in Theme::ALL {
            for color in [false, true] {
                let lines = render_dashboard_lines_colored(Some(&boundary_cfg), *theme, color);
                for (idx, line) in lines.iter().enumerate() {
                    let vis = visible_len(line);
                    assert_eq!(
                        vis, 78,
                        "Boundary config produced line {idx} with width {vis} != 78 (theme={:?}, color={color}):\n{line}",
                        theme
                    );
                }
            }
        }

        // 7. Pure render_dashboard_lines default caller test
        for theme in Theme::ALL {
            let lines = render_dashboard_lines(Some(&boundary_cfg), *theme);
            for (idx, line) in lines.iter().enumerate() {
                assert_eq!(
                    visible_len(line),
                    78,
                    "Pure dashboard line {idx} width != 78 for theme {:?}:\n{line}",
                    theme
                );
            }
        }

        // 8. Hermes single-line tool categories and balanced row layout (no multiline slop)
        let rendered = render_dashboard_lines_colored(Some(&boundary_cfg), Theme::PureBlack, false);
        assert_eq!(rendered.len(), 15);
        assert!(rendered[2].contains("Academics due, attendance, assignments, grades"));
        assert!(rendered[3].contains("Materials materials, material"));
        assert!(rendered[4].contains("Records   classes, account, search"));
        assert!(rendered[5].contains("Agentic   submit_assignment, submit_quiz, forms"));
        assert!(rendered[1].contains("██████╗"));
        assert!(rendered[8].contains("Claude ("));
        assert!(rendered[9].contains("Gemini ("));
    }
}
