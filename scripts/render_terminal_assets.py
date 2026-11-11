#!/usr/bin/env python3
import argparse
from dataclasses import dataclass
import html
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Dict, List, Optional, Tuple

CHROME_BIN = (
    shutil.which("google-chrome")
    or shutil.which("chromium")
    or (
        os.path.exists("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
        and "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
    )
    or "google-chrome"
)
FFMPEG_BIN = (
    shutil.which("ffmpeg")
    or (os.path.exists("/etc/profiles/per-user/MacbookPro/bin/ffmpeg") and "/etc/profiles/per-user/MacbookPro/bin/ffmpeg")
    or "ffmpeg"
)
OUTPUT_DIR = os.path.abspath("assets")
SCREENSHOT_DIR = os.path.join(OUTPUT_DIR, "screenshots")
VIDEO_DIR = os.path.join(OUTPUT_DIR, "videos")

os.makedirs(SCREENSHOT_DIR, exist_ok=True)
os.makedirs(VIDEO_DIR, exist_ok=True)

DIGI_BIN = os.path.abspath("./target/release/digi")
if not os.path.exists(DIGI_BIN):
    debug_bin = os.path.abspath("./target/debug/digi")
    if os.path.exists(debug_bin):
        DIGI_BIN = debug_bin
    elif shutil.which("digi"):
        DIGI_BIN = shutil.which("digi")

@dataclass
class TerminalTheme:
    name: str
    slug: str
    window_bg: str
    titlebar_bg: str
    titlebar_border: str
    title_color: str
    box_shadow: str
    text_color: str
    prompt_symbol: str
    prompt_path: str
    prompt_cmd: str
    cursor_color: str
    badge_ok: str
    badge_info: str
    badge_step: str
    ansi_34: str
    cli_theme: str

THEMES: Dict[str, TerminalTheme] = {
    "pure-black": TerminalTheme(
        name="Pure Black",
        slug="pure-black",
        window_bg="#000000",
        titlebar_bg="#0c0d0e",
        titlebar_border="1px solid rgba(255, 255, 255, 0.12)",
        title_color="#9ca3af",
        box_shadow="0 24px 64px rgba(0, 0, 0, 0.95), 0 0 0 1px rgba(255, 255, 255, 0.16)",
        text_color="#f3f4f6",
        prompt_symbol="#ffffff",
        prompt_path="#9ca3af",  # Crisp neutral silver slate, no blue!
        prompt_cmd="#ffffff",
        cursor_color="#ffffff",
        badge_ok="#22c55e",
        badge_info="#6b7280",
        badge_step="#ffffff",
        ansi_34="#d1d5db",
        cli_theme="pure-black",
    ),
    "tokyo-night": TerminalTheme(
        name="Tokyo Night / Midnight",
        slug="tokyo-night",
        window_bg="#1a1b26",
        titlebar_bg="#16161e",
        titlebar_border="1px solid rgba(122, 162, 247, 0.20)",
        title_color="#7aa2f7",
        box_shadow="0 24px 64px rgba(0, 0, 0, 0.85), 0 0 0 1px rgba(122, 162, 247, 0.25), 0 0 32px rgba(122, 162, 247, 0.08)",
        text_color="#c0caf5",
        prompt_symbol="#bb9af7",
        prompt_path="#7aa2f7",
        prompt_cmd="#c0caf5",
        cursor_color="#c0caf5",
        badge_ok="#9ece6a",
        badge_info="#565f89",
        badge_step="#bb9af7",
        ansi_34="#7aa2f7",
        cli_theme="tokyo-night",
    ),
    "crimson-dark": TerminalTheme(
        name="Crimson Dark / Digii",
        slug="crimson-dark",
        window_bg="#0f0507",
        titlebar_bg="#18080b",
        titlebar_border="1px solid rgba(239, 68, 68, 0.20)",
        title_color="#e06c75",
        box_shadow="0 24px 64px rgba(0, 0, 0, 0.85), 0 0 0 1px rgba(220, 38, 38, 0.28), 0 0 32px rgba(220, 38, 38, 0.08)",
        text_color="#f5e6e8",
        prompt_symbol="#ef4444",
        prompt_path="#f87171",  # Digii light coral/scarlet, no blue!
        prompt_cmd="#ffffff",
        cursor_color="#ef4444",
        badge_ok="#22c55e",
        badge_info="#9ca3af",
        badge_step="#ef4444",
        ansi_34="#f87171",
        cli_theme="crimson-dark",
    ),
    "matrix-emerald": TerminalTheme(
        name="Cyberpunk / Matrix Emerald",
        slug="matrix-emerald",
        window_bg="#050d08",
        titlebar_bg="#0b170f",
        titlebar_border="1px solid rgba(16, 185, 129, 0.25)",
        title_color="#34d399",
        box_shadow="0 24px 64px rgba(0, 0, 0, 0.85), 0 0 0 1px rgba(16, 185, 129, 0.35), 0 0 36px rgba(16, 185, 129, 0.12)",
        text_color="#ecfdf5",
        prompt_symbol="#10b981",
        prompt_path="#34d399",  # Bright matrix emerald, no blue!
        prompt_cmd="#a7f3d0",
        cursor_color="#34d399",
        badge_ok="#34d399",
        badge_info="#4b7863",
        badge_step="#10b981",
        ansi_34="#34d399",
        cli_theme="matrix-emerald",
    ),
}

# Semantic aliases
THEMES["cyberpunk"] = THEMES["matrix-emerald"]
THEMES["midnight"] = THEMES["tokyo-night"]
THEMES["digii"] = THEMES["crimson-dark"]
THEMES["pure"] = THEMES["pure-black"]
THEMES["pure_black"] = THEMES["pure-black"]
THEMES["pureblack"] = THEMES["pure-black"]
THEMES["tokyo_night"] = THEMES["tokyo-night"]
THEMES["tokyonight"] = THEMES["tokyo-night"]
THEMES["crimson_dark"] = THEMES["crimson-dark"]
THEMES["crimsondark"] = THEMES["crimson-dark"]
THEMES["matrix_emerald"] = THEMES["matrix-emerald"]
THEMES["matrixemerald"] = THEMES["matrix-emerald"]

CORE_THEME_KEYS = ["pure-black", "tokyo-night", "crimson-dark", "matrix-emerald"]

def ansi_to_html(ansi_str: str, theme: Optional[TerminalTheme] = None) -> str:
    """Convert ANSI escape sequences to styled HTML spans."""
    pos = 0
    out = []
    current_styles = []

    pattern = re.compile(r'\x1b\[([0-9;]*)([a-zA-Z])')
    blue_replacement = theme.ansi_34 if theme else "#d1d5db"

    for match in pattern.finditer(ansi_str):
        chunk = ansi_str[pos:match.start()]
        if chunk:
            escaped = html.escape(chunk)
            if current_styles:
                style_str = "; ".join(current_styles)
                out.append(f'<span style="{style_str}">{escaped}</span>')
            else:
                out.append(escaped)

        cmd = match.group(2)
        if cmd == 'm':
            code_str = match.group(1)
            if not code_str or code_str == "0":
                current_styles = []
            else:
                codes = [int(c) for c in code_str.split(";") if c.isdigit()]
                idx = 0
                while idx < len(codes):
                    c = codes[idx]
                    if c == 0:
                        current_styles = []
                    elif c == 1:
                        current_styles.append("font-weight: 700")
                    elif c == 2:
                        current_styles.append("opacity: 0.65")
                    elif c == 30:
                        current_styles.append("color: #4b5563")
                    elif c == 31:
                        current_styles.append("color: #ef4444")
                    elif c == 32:
                        current_styles.append("color: #22c55e")
                    elif c == 33:
                        current_styles.append("color: #eab308")
                    elif c == 34:
                        current_styles.append(f"color: {blue_replacement}")
                    elif c == 35:
                        current_styles.append("color: #a855f7")
                    elif c == 36:
                        current_styles.append("color: #06b6d4")
                    elif c == 37:
                        current_styles.append("color: #f3f4f6")
                    elif c == 90:
                        current_styles.append("color: #9ca3af")
                    elif c == 38 and idx + 4 < len(codes) and codes[idx + 1] == 2:
                        r, g, b = codes[idx + 2], codes[idx + 3], codes[idx + 4]
                        current_styles.append(f"color: rgb({r}, {g}, {b})")
                        idx += 4
                    idx += 1
        pos = match.end()

    chunk = ansi_str[pos:]
    if chunk:
        escaped = html.escape(chunk)
        if current_styles:
            style_str = "; ".join(current_styles)
            out.append(f'<span style="{style_str}">{escaped}</span>')
        else:
            out.append(escaped)

    return "".join(out)

def render_terminal_window(title: str, content_html: str, theme: Optional[TerminalTheme] = None, is_video: bool = False) -> str:
    if theme is None:
        theme = THEMES["pure-black"]
    body_bg = theme.window_bg if is_video else "transparent"
    box_shadow = theme.box_shadow
    return f"""<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  * {{
    box-sizing: border-box;
    margin: 0;
    padding: 0;
  }}
  body {{
    background: {body_bg};
    padding: 24px;
    font-family: ui-monospace, "SF Mono", "JetBrains Mono", Menlo, Monaco, Consolas, monospace;
    display: flex;
    justify-content: center;
    align-items: center;
    min-height: 100vh;
  }}
  .window {{
    width: 880px;
    background: {theme.window_bg};
    border-radius: 12px;
    box-shadow: {box_shadow};
    overflow: hidden;
  }}
  .titlebar {{
    background: {theme.titlebar_bg};
    padding: 12px 16px;
    display: flex;
    align-items: center;
    border-bottom: {theme.titlebar_border};
    position: relative;
  }}
  .traffic-lights {{
    display: flex;
    gap: 8px;
  }}
  .light {{
    width: 12px;
    height: 12px;
    border-radius: 50%;
  }}
  .red {{ background: #ff5f56; }}
  .yellow {{ background: #ffbd2e; }}
  .green {{ background: #27c93f; }}
  .title {{
    position: absolute;
    left: 0;
    right: 0;
    text-align: center;
    color: {theme.title_color};
    font-size: 12px;
    font-weight: 500;
    pointer-events: none;
  }}
  .terminal-body {{
    padding: 22px 26px;
    color: {theme.text_color};
    font-size: 13.5px;
    line-height: 1.48;
    white-space: pre;
    tab-size: 4;
    min-height: 400px;
  }}
  .prompt-row {{
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 10px;
  }}
  .prompt-symbol {{
    color: {theme.prompt_symbol};
    font-weight: bold;
  }}
  .prompt-path {{
    color: {theme.prompt_path};
    font-weight: 500;
  }}
  .prompt-cmd {{
    color: {theme.prompt_cmd};
    font-weight: 600;
  }}
  .cursor {{
    display: inline-block;
    width: 8px;
    height: 15px;
    background: {theme.cursor_color};
    margin-left: 2px;
    vertical-align: -2px;
  }}
  .badge-ok {{
    color: {theme.badge_ok};
    font-weight: bold;
  }}
  .badge-info {{
    color: {theme.badge_info};
  }}
  .badge-step {{
    color: {theme.badge_step};
    font-weight: bold;
  }}
</style>
</head>
<body>
  <div class="window">
    <div class="titlebar">
      <div class="traffic-lights">
        <div class="light red"></div>
        <div class="light yellow"></div>
        <div class="light green"></div>
      </div>
      <div class="title">{title}</div>
    </div>
    <div class="terminal-body">{content_html}</div>
  </div>
</body>
</html>"""

def capture_html_to_png(html_content: str, output_png: str, width: int = 960, height: int = 680, is_video: bool = False):
    with tempfile.NamedTemporaryFile("w", suffix=".html", delete=False) as f:
        f.write(html_content)
        tmp_html = f.name

    bg_color = "000000ff" if is_video else "00000000"
    cmd = [
        CHROME_BIN,
        "--headless=new",
        "--disable-gpu",
        f"--window-size={width},{height}",
        f"--default-background-color={bg_color}",
        "--device-scale-factor=2",
        f"--screenshot={output_png}",
        f"file://{tmp_html}"
    ]
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    os.remove(tmp_html)

def build_video_from_frames(frames: list[tuple[str, float]], output_base: str, title: str, theme: Optional[TerminalTheme] = None, width: int = 960, height: int = 660, alias_base: str = None):
    """Render a list of (html_body, duration_seconds) into crisp, high-quality GIF and MP4."""
    if theme is None:
        theme = THEMES["pure-black"]
    tmp_dir = tempfile.mkdtemp(prefix="digi_video_")
    try:
        concat_list = []
        for idx, (body_html, duration) in enumerate(frames):
            frame_png = os.path.join(tmp_dir, f"frame_{idx:04d}.png")
            html_doc = render_terminal_window(title, body_html, theme=theme, is_video=True)
            capture_html_to_png(html_doc, frame_png, width=width, height=height, is_video=True)
            concat_list.append((frame_png, duration))

        # Write ffmpeg concat file
        concat_file = os.path.join(tmp_dir, "input.txt")
        with open(concat_file, "w") as f:
            for png_path, dur in concat_list:
                f.write(f"file '{png_path}'\n")
                f.write(f"duration {dur}\n")
            if concat_list:
                f.write(f"file '{concat_list[-1][0]}'\n")

        gif_out = f"{output_base}.gif"
        mp4_out = f"{output_base}.mp4"

        # 1. High-Quality 20fps Palette-Optimized GIF (crisp text, no banding)
        palette_png = os.path.join(tmp_dir, "palette.png")
        cmd_palette = [
            FFMPEG_BIN, "-y", "-f", "concat", "-safe", "0", "-i", concat_file,
            "-vf", "fps=20,scale=960:-1:flags=lanczos,palettegen=max_colors=256:stats_mode=diff",
            palette_png
        ]
        subprocess.run(cmd_palette, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        cmd_gif = [
            FFMPEG_BIN, "-y", "-f", "concat", "-safe", "0", "-i", concat_file,
            "-i", palette_png,
            "-lavfi", "fps=20,scale=960:-1:flags=lanczos [x]; [x][1:v] paletteuse=dither=sierra2_4a",
            gif_out
        ]
        subprocess.run(cmd_gif, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        # 2. Crystal-Clear 30fps H.264 MP4 (cleanly clipped edges, studio backdrop)
        cmd_mp4 = [
            FFMPEG_BIN, "-y", "-f", "concat", "-safe", "0", "-i", concat_file,
            "-vf", "fps=30,format=yuv420p,scale=1280:-2:flags=lanczos",
            "-c:v", "libx264", "-profile:v", "high", "-preset", "slow", "-crf", "14", "-pix_fmt", "yuv420p",
            mp4_out
        ]
        subprocess.run(cmd_mp4, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        print(f"  ✔ Video created: {gif_out}")
        print(f"  ✔ Video created: {mp4_out}")

        if alias_base:
            shutil.copy2(gif_out, f"{alias_base}.gif")
            shutil.copy2(mp4_out, f"{alias_base}.mp4")
            print(f"  ✔ Mirrored to: {alias_base}.gif & .mp4")
    finally:
        shutil.rmtree(tmp_dir, ignore_errors=True)

def generate_screenshots(target_themes: Optional[List[str]] = None):
    print("==> 1. Generating Screenshots for Theme Variants...")
    keys = target_themes if target_themes else CORE_THEME_KEYS

    for key in keys:
        theme = THEMES.get(key)
        if not theme:
            print(f"  ✖ Unknown theme key: {key}")
            continue

        env = {
            **dict(os.environ),
            "FORCE_COLOR": "1",
            "DIGIMCP_THEME": theme.cli_theme,
        }

        # Render theme dashboard screenshot
        cmd = [DIGI_BIN, "dashboard", "--theme", theme.cli_theme]
        dashboard_raw = subprocess.check_output(cmd, env=env).decode("utf-8")
        dashboard_html = ansi_to_html(dashboard_raw, theme)
        prompt_html = (
            f'<div class="prompt-row">'
            f'<span class="prompt-symbol">❯</span> '
            f'<span class="prompt-path">~/oss/DigiMCP</span> '
            f'<span class="prompt-cmd">digi dashboard --theme {theme.cli_theme}</span>'
            f'</div>'
        )
        dashboard_body = prompt_html + dashboard_html
        doc = render_terminal_window(
            f"kushvint@macaroon: ~/oss/DigiMCP — digi dashboard ({theme.name})",
            dashboard_body,
            theme=theme,
            is_video=False,
        )
        theme_png = os.path.join(SCREENSHOT_DIR, f"theme-{theme.slug}.png")
        capture_html_to_png(doc, theme_png, width=960, height=720, is_video=False)
        print(f"  ✔ Theme Preview ({theme.name}): {theme_png}")

        if theme.slug == "matrix-emerald":
            cyberpunk_alias = os.path.join(SCREENSHOT_DIR, "theme-cyberpunk.png")
            shutil.copy2(theme_png, cyberpunk_alias)
            print(f"  ✔ Mirrored alias: {cyberpunk_alias}")

        # Update default primary launch dashboard using Pure Black
        if theme.slug == "pure-black":
            default_dash_png = os.path.join(SCREENSHOT_DIR, "digimcp-launch-dashboard.png")
            default_prompt = (
                '<div class="prompt-row">'
                '<span class="prompt-symbol">❯</span> '
                '<span class="prompt-path">~/oss/DigiMCP</span> '
                '<span class="prompt-cmd">digi dashboard</span>'
                '</div>'
            )
            doc_default = render_terminal_window(
                "kushvint@macaroon: ~/oss/DigiMCP — digi dashboard",
                default_prompt + dashboard_html,
                theme=theme,
                is_video=False,
            )
            capture_html_to_png(doc_default, default_dash_png, width=960, height=720, is_video=False)
            shutil.copy2(default_dash_png, os.path.join(SCREENSHOT_DIR, "quad-launch-dashboard.png"))
            print(f"  ✔ Primary Dashboard: {default_dash_png}")

    # Generate Doctor Screenshot with Pure Black theme
    pb_theme = THEMES["pure-black"]
    env_pb = {
        **dict(os.environ),
        "FORCE_COLOR": "1",
        "DIGIMCP_THEME": pb_theme.cli_theme,
    }
    doctor_raw = subprocess.check_output([DIGI_BIN, "doctor"], env=env_pb).decode("utf-8")
    doctor_html = ansi_to_html(doctor_raw, pb_theme)
    doctor_body = (
        '<div class="prompt-row"><span class="prompt-symbol">❯</span> <span class="prompt-path">~/oss/DigiMCP</span> <span class="prompt-cmd">digi doctor</span></div>'
        + doctor_html
    )
    doc_doctor = render_terminal_window("kushvint@macaroon: ~/oss/DigiMCP — digi doctor", doctor_body, theme=pb_theme, is_video=False)
    out_doctor = os.path.join(SCREENSHOT_DIR, "digimcp-doctor.png")
    capture_html_to_png(doc_doctor, out_doctor, width=960, height=520, is_video=False)
    shutil.copy2(out_doctor, os.path.join(SCREENSHOT_DIR, "quad-doctor.png"))
    print(f"  ✔ Diagnostics Screenshot: {out_doctor}")

def generate_usage_video(theme: Optional[TerminalTheme] = None):
    print("==> 2. Generating Tool Usage Video (digi doctor & intelligence)...")
    if theme is None:
        theme = THEMES["pure-black"]
    env = {
        **dict(os.environ),
        "FORCE_COLOR": "1",
        "DIGIMCP_THEME": theme.cli_theme,
    }
    doctor_raw = subprocess.check_output([DIGI_BIN, "doctor"], env=env).decode("utf-8")
    doctor_html = ansi_to_html(doctor_raw, theme)

    prompt_prefix = '<div class="prompt-row"><span class="prompt-symbol">❯</span> <span class="prompt-path">~/oss/DigiMCP</span> <span class="prompt-cmd">'

    cmd_text = "digi doctor"
    frames = []

    # Frame 0: Empty prompt
    frames.append((f"{prompt_prefix}<span class=\"cursor\"></span></span></div>", 0.6))

    # Typing frames
    for i in range(1, len(cmd_text) + 1):
        typed = cmd_text[:i]
        frames.append((f"{prompt_prefix}{typed}<span class=\"cursor\"></span></span></div>", 0.12))

    # Enter pressed (brief pause)
    frames.append((f"{prompt_prefix}{cmd_text}</span></div>", 0.3))

    # Partial execution: checkmark lines appearing progressively
    lines = doctor_html.split("\n")
    for i in range(2, len(lines) + 1):
        step_output = "\n".join(lines[:i])
        frame_content = (
            f"{prompt_prefix}{cmd_text}</span></div>"
            f"{step_output}"
        )
        frames.append((frame_content, 0.18))

    # Hold final result
    final_content = (
        f"{prompt_prefix}{cmd_text}</span></div>"
        f"{doctor_html}\n"
        f'<div class="prompt-row"><span class="prompt-symbol">❯</span> <span class="prompt-path">~/oss/DigiMCP</span> <span class="cursor"></span></div>'
    )
    frames.append((final_content, 3.5))

    out_base = os.path.join(VIDEO_DIR, "digimcp-usage")
    alias_base = os.path.join(VIDEO_DIR, "quad-usage")
    build_video_from_frames(frames, out_base, "kushvint@macaroon: ~/oss/DigiMCP — digi doctor", theme=theme, width=960, height=560, alias_base=alias_base)

def generate_setup_video(theme: Optional[TerminalTheme] = None):
    print("==> 3. Generating Agent Integration Video (digi setup)...")
    if theme is None:
        theme = THEMES["pure-black"]
    prompt_prefix = '<div class="prompt-row"><span class="prompt-symbol">❯</span> <span class="prompt-path">~/oss/DigiMCP</span> <span class="prompt-cmd">'

    cmd_text = "digi setup"
    frames = []

    # Frame 0: Empty prompt
    frames.append((f"{prompt_prefix}<span class=\"cursor\"></span></span></div>", 0.6))

    # Typing digi setup
    for i in range(1, len(cmd_text) + 1):
        typed = cmd_text[:i]
        frames.append((f"{prompt_prefix}{typed}<span class=\"cursor\"></span></span></div>", 0.12))

    frames.append((f"{prompt_prefix}{cmd_text}</span></div>", 0.3))

    # Discovery summary
    discovery_html = (
        f"{prompt_prefix}{cmd_text}</span></div>\n"
        '  <span class="badge-step">›</span> digi setup: about to configure 9 item(s) across 5 detected agent(s):\n\n'
        '  <span class="badge-info">•</span> Claude Code  run     claude mcp add --scope user digimcp -- ~/.local/bin/digi\n'
        '  <span class="badge-info">•</span> Claude Code  skill   ~/.claude/skills/digimcp/SKILL.md\n'
        '  <span class="badge-info">•</span> Codex        edit    ~/.codex/config.toml  [mcp_servers.digimcp]\n'
        '  <span class="badge-info">•</span> Codex        skill   ~/.codex/skills/digimcp/SKILL.md\n'
        '  <span class="badge-info">•</span> Cursor       edit    ~/.cursor/mcp.json  ["digimcp"]\n'
        '  <span class="badge-info">•</span> Cursor       skill   ~/.cursor/skills/digimcp/SKILL.md\n'
        '  <span class="badge-info">•</span> Gemini CLI   edit    ~/.gemini/settings.json  ["digimcp"]\n'
        '  <span class="badge-info">•</span> opencode     edit    ~/.config/opencode/opencode.json  ["digimcp"]\n'
        '  <span class="badge-info">•</span> opencode     skill   ~/.config/opencode/skills/digimcp/SKILL.md\n\n'
        '  <span class="badge-step">›</span> Apply changes? [y/N] <span class="cursor"></span>'
    )
    frames.append((discovery_html, 1.2))

    # User types 'y'
    applied_prompt = discovery_html.replace('<span class="cursor"></span>', 'y\n')
    frames.append((applied_prompt, 0.4))

    # Progressive apply steps
    apply_steps = [
        '  <span class="badge-ok">✔</span> Claude Code  claude mcp add succeeded',
        '  <span class="badge-ok">✔</span> Claude Code  copied skill to ~/.claude/skills/digimcp/SKILL.md',
        '  <span class="badge-ok">✔</span> Codex        updated ~/.codex/config.toml',
        '  <span class="badge-ok">✔</span> Codex        copied skill to ~/.codex/skills/digimcp/SKILL.md',
        '  <span class="badge-ok">✔</span> Cursor       updated ~/.cursor/mcp.json',
        '  <span class="badge-ok">✔</span> Cursor       copied skill to ~/.cursor/skills/digimcp/SKILL.md',
        '  <span class="badge-ok">✔</span> Gemini CLI   updated ~/.gemini/settings.json',
        '  <span class="badge-ok">✔</span> opencode     updated ~/.config/opencode/opencode.json',
        '  <span class="badge-ok">✔</span> opencode     copied skill to ~/.config/opencode/skills/digimcp/SKILL.md',
    ]

    for i in range(1, len(apply_steps) + 1):
        step_body = applied_prompt + "\n" + "\n".join(apply_steps[:i])
        frames.append((step_body, 0.22))

    # Final completion message
    final_body = (
        applied_prompt + "\n" + "\n".join(apply_steps) + "\n\n"
        '  <span class="badge-ok">✔</span> digi successfully wired into your agents!\n\n'
        '  <span class="badge-step">›</span> Next steps:\n'
        '    1. Restart your agent (Claude Code, Codex, Cursor, Gemini CLI, or OpenCode).\n'
        '    2. Try asking: "what\'s due this week?" or "check my attendance".\n'
        '    3. Run `digi doctor` anytime to verify connection health.\n\n'
        '<div class="prompt-row"><span class="prompt-symbol">❯</span> <span class="prompt-path">~/oss/DigiMCP</span> <span class="cursor"></span></div>'
    )
    frames.append((final_body, 4.0))

    out_base = os.path.join(VIDEO_DIR, "digimcp-setup")
    alias_base = os.path.join(VIDEO_DIR, "quad-setup")
    build_video_from_frames(frames, out_base, "kushvint@macaroon: ~/oss/DigiMCP — digi setup", theme=theme, width=960, height=640, alias_base=alias_base)

def normalize_theme_arg(val: str) -> str:
    cleaned = val.strip().lower().replace("_", "-")
    if cleaned in THEMES:
        return cleaned
    compact = cleaned.replace("-", "")
    if compact in THEMES:
        return compact
    raise argparse.ArgumentTypeError(
        f"Unknown theme: '{val}'. Available themes: {', '.join(CORE_THEME_KEYS)}"
    )

def main():
    parser = argparse.ArgumentParser(description="Render terminal visual assets for DigiMCP.")
    parser.add_argument("--screenshots-only", "--screenshots", action="store_true", help="Generate only screenshot assets.")
    parser.add_argument("--videos-only", "--videos", action="store_true", help="Generate only video assets.")
    parser.add_argument("--theme", type=normalize_theme_arg, help="Target specific theme for generation (pure-black, tokyo-night, crimson-dark, matrix-emerald).")
    args = parser.parse_args()

    print("==> Building visual asset suite for DigiMCP...")
    target_themes = [args.theme] if args.theme else None

    if args.videos_only:
        sel_theme = THEMES.get(args.theme, THEMES["pure-black"])
        generate_usage_video(sel_theme)
        generate_setup_video(sel_theme)
    elif args.screenshots_only:
        generate_screenshots(target_themes)
    else:
        generate_screenshots(target_themes)
        sel_theme = THEMES.get(args.theme, THEMES["pure-black"])
        generate_usage_video(sel_theme)
        generate_setup_video(sel_theme)

    print("==> Visual asset generation complete!")

if __name__ == "__main__":
    main()
