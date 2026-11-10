#!/usr/bin/env python3
import html
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

CHROME_BIN = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
FFMPEG_BIN = "/etc/profiles/per-user/MacbookPro/bin/ffmpeg"
OUTPUT_DIR = os.path.abspath("assets")
SCREENSHOT_DIR = os.path.join(OUTPUT_DIR, "screenshots")
VIDEO_DIR = os.path.join(OUTPUT_DIR, "videos")

os.makedirs(SCREENSHOT_DIR, exist_ok=True)
os.makedirs(VIDEO_DIR, exist_ok=True)

def ansi_to_html(ansi_str: str) -> str:
    """Convert ANSI escape sequences to styled HTML spans."""
    pos = 0
    out = []
    current_styles = []

    pattern = re.compile(r'\x1b\[([0-9;]*)m')

    for match in pattern.finditer(ansi_str):
        chunk = ansi_str[pos:match.start()]
        if chunk:
            escaped = html.escape(chunk)
            if current_styles:
                style_str = "; ".join(current_styles)
                out.append(f'<span style="{style_str}">{escaped}</span>')
            else:
                out.append(escaped)

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
                    current_styles.append("color: #3b82f6")
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

def render_terminal_window(title: str, content_html: str) -> str:
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
    background: transparent;
    padding: 24px;
    font-family: ui-monospace, "SF Mono", "JetBrains Mono", Menlo, Monaco, Consolas, monospace;
    display: flex;
    justify-content: center;
    align-items: center;
    min-height: 100vh;
  }}
  .window {{
    width: 860px;
    background: #0d1117;
    border-radius: 12px;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.7), 0 0 0 1px rgba(255, 255, 255, 0.08);
    overflow: hidden;
  }}
  .titlebar {{
    background: #161b22;
    padding: 12px 16px;
    display: flex;
    align-items: center;
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
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
    color: #8b949e;
    font-size: 12px;
    font-weight: 500;
    pointer-events: none;
  }}
  .terminal-body {{
    padding: 22px 26px;
    color: #e6edf3;
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
    color: #ef4444;
    font-weight: bold;
  }}
  .prompt-path {{
    color: #38bdf8;
    font-weight: 500;
  }}
  .prompt-cmd {{
    color: #f0f6fc;
    font-weight: 600;
  }}
  .cursor {{
    display: inline-block;
    width: 8px;
    height: 15px;
    background: #f0f6fc;
    margin-left: 2px;
    vertical-align: -2px;
  }}
  .badge-ok {{
    color: #22c55e;
    font-weight: bold;
  }}
  .badge-info {{
    color: #9ca3af;
  }}
  .badge-step {{
    color: #ef4444;
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

def capture_html_to_png(html_content: str, output_png: str, width: int = 940, height: int = 680):
    with tempfile.NamedTemporaryFile("w", suffix=".html", delete=False) as f:
        f.write(html_content)
        tmp_html = f.name

    cmd = [
        CHROME_BIN,
        "--headless=new",
        "--disable-gpu",
        f"--window-size={width},{height}",
        "--default-background-color=00000000",
        "--device-scale-factor=2",
        f"--screenshot={output_png}",
        f"file://{tmp_html}"
    ]
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    os.remove(tmp_html)

def build_video_from_frames(frames: list[tuple[str, float]], output_base: str, title: str, width: int = 940, height: int = 640):
    """Render a list of (html_body, duration_seconds) into both GIF and MP4 via ffmpeg."""
    tmp_dir = tempfile.mkdtemp(prefix="quad_video_")
    try:
        concat_list = []
        for idx, (body_html, duration) in enumerate(frames):
            frame_png = os.path.join(tmp_dir, f"frame_{idx:04d}.png")
            html_doc = render_terminal_window(title, body_html)
            capture_html_to_png(html_doc, frame_png, width=width, height=height)
            concat_list.append((frame_png, duration))

        # Write ffmpeg concat file
        concat_file = os.path.join(tmp_dir, "input.txt")
        with open(concat_file, "w") as f:
            for png_path, dur in concat_list:
                f.write(f"file '{png_path}'\n")
                f.write(f"duration {dur}\n")
            # Last frame needs duplicate entry per ffmpeg concat spec
            if concat_list:
                f.write(f"file '{concat_list[-1][0]}'\n")

        gif_out = f"{output_base}.gif"
        mp4_out = f"{output_base}.mp4"

        # 1. High-Quality Palette-Optimized GIF
        palette_png = os.path.join(tmp_dir, "palette.png")
        cmd_palette = [
            FFMPEG_BIN, "-y", "-f", "concat", "-safe", "0", "-i", concat_file,
            "-vf", "fps=10,scale=860:-1:flags=lanczos,palettegen=stats_mode=diff",
            palette_png
        ]
        subprocess.run(cmd_palette, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        cmd_gif = [
            FFMPEG_BIN, "-y", "-f", "concat", "-safe", "0", "-i", concat_file,
            "-i", palette_png,
            "-lavfi", "fps=10,scale=860:-1:flags=lanczos [x]; [x][1:v] paletteuse=dither=bayer:bayer_scale=3",
            gif_out
        ]
        subprocess.run(cmd_gif, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        # 2. Modern MP4 (H.264)
        cmd_mp4 = [
            FFMPEG_BIN, "-y", "-f", "concat", "-safe", "0", "-i", concat_file,
            "-vf", "fps=25,format=yuv420p,scale=860:-2",
            "-c:v", "libx264", "-profile:v", "high", "-crf", "18", "-pix_fmt", "yuv420p",
            mp4_out
        ]
        subprocess.run(cmd_mp4, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        print(f"  ✔ Video created: {gif_out}")
        print(f"  ✔ Video created: {mp4_out}")
    finally:
        shutil.rmtree(tmp_dir, ignore_errors=True)

def generate_screenshots():
    print("==> 1. Generating Screenshots...")
    env = {**dict(os.environ), "FORCE_COLOR": "1"}

    # Dashboard Screenshot (Application Launching)
    dashboard_raw = subprocess.check_output(["./target/debug/quad", "dashboard"], env=env).decode("utf-8")
    dashboard_html = ansi_to_html(dashboard_raw)
    dashboard_body = (
        '<div class="prompt-row"><span class="prompt-symbol">❯</span> <span class="prompt-path">~/oss/quad</span> <span class="prompt-cmd">quad dashboard</span></div>'
        + dashboard_html
    )
    doc = render_terminal_window("kushvinth@macbook: ~/oss/quad — quad dashboard", dashboard_body)
    out_png = os.path.join(SCREENSHOT_DIR, "quad-launch-dashboard.png")
    capture_html_to_png(doc, out_png, width=960, height=720)
    print(f"  ✔ Screenshot: {out_png}")

    # Doctor Screenshot
    doctor_raw = subprocess.check_output(["./target/debug/quad", "doctor"], env=env).decode("utf-8")
    doctor_html = ansi_to_html(doctor_raw)
    doctor_body = (
        '<div class="prompt-row"><span class="prompt-symbol">❯</span> <span class="prompt-path">~/oss/quad</span> <span class="prompt-cmd">quad doctor</span></div>'
        + doctor_html
    )
    doc_doctor = render_terminal_window("kushvinth@macbook: ~/oss/quad — quad doctor", doctor_body)
    out_doctor = os.path.join(SCREENSHOT_DIR, "quad-doctor.png")
    capture_html_to_png(doc_doctor, out_doctor, width=960, height=520)
    print(f"  ✔ Screenshot: {out_doctor}")

def generate_usage_video():
    print("==> 2. Generating Tool Usage Video (quad doctor & intelligence)...")
    env = {**dict(os.environ), "FORCE_COLOR": "1"}
    doctor_raw = subprocess.check_output(["./target/debug/quad", "doctor"], env=env).decode("utf-8")
    doctor_html = ansi_to_html(doctor_raw)

    prompt_prefix = '<div class="prompt-row"><span class="prompt-symbol">❯</span> <span class="prompt-path">~/oss/quad</span> <span class="prompt-cmd">'

    cmd_text = "quad doctor"
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
        f'<div class="prompt-row"><span class="prompt-symbol">❯</span> <span class="prompt-path">~/oss/quad</span> <span class="cursor"></span></div>'
    )
    frames.append((final_content, 3.5))

    out_base = os.path.join(VIDEO_DIR, "quad-usage")
    build_video_from_frames(frames, out_base, "kushvinth@macbook: ~/oss/quad — quad doctor", width=960, height=540)

def generate_setup_video():
    print("==> 3. Generating Agent Integration Video (quad setup)...")
    prompt_prefix = '<div class="prompt-row"><span class="prompt-symbol">❯</span> <span class="prompt-path">~/oss/quad</span> <span class="prompt-cmd">'

    cmd_text = "quad setup"
    frames = []

    # Frame 0: Empty prompt
    frames.append((f"{prompt_prefix}<span class=\"cursor\"></span></span></div>", 0.6))

    # Typing quad setup
    for i in range(1, len(cmd_text) + 1):
        typed = cmd_text[:i]
        frames.append((f"{prompt_prefix}{typed}<span class=\"cursor\"></span></span></div>", 0.12))

    frames.append((f"{prompt_prefix}{cmd_text}</span></div>", 0.3))

    # Discovery summary
    discovery_html = (
        f"{prompt_prefix}{cmd_text}</span></div>\n"
        '  <span class="badge-step">›</span> quad setup: about to configure 9 item(s) across 5 detected agent(s):\n\n'
        '  <span class="badge-info">•</span> Claude Code  run     claude mcp add --scope user quad -- ~/.local/bin/quad\n'
        '  <span class="badge-info">•</span> Claude Code  skill   ~/.claude/skills/quad/SKILL.md\n'
        '  <span class="badge-info">•</span> Codex        edit    ~/.codex/config.toml  [mcp_servers.quad]\n'
        '  <span class="badge-info">•</span> Cursor       edit    ~/.cursor/mcp.json  ["quad"]\n'
        '  <span class="badge-info">•</span> Gemini CLI   edit    ~/.gemini/settings.json  ["quad"]\n'
        '  <span class="badge-info">•</span> opencode     edit    ~/.config/opencode/opencode.json  ["quad"]\n'
        '  <span class="badge-info">•</span> opencode     skill   ~/.config/opencode/skills/quad/SKILL.md\n\n'
        '  <span class="badge-step">›</span> Apply changes? [y/N] <span class="cursor"></span>'
    )
    frames.append((discovery_html, 1.2))

    # User types 'y'
    applied_prompt = discovery_html.replace('<span class="cursor"></span>', 'y\n')
    frames.append((applied_prompt, 0.4))

    # Progressive apply steps
    apply_steps = [
        '  <span class="badge-ok">✔</span> Claude Code  claude mcp add succeeded',
        '  <span class="badge-ok">✔</span> Claude Code  copied skill to ~/.claude/skills/quad/SKILL.md',
        '  <span class="badge-ok">✔</span> Codex        updated ~/.codex/config.toml',
        '  <span class="badge-ok">✔</span> Cursor       updated ~/.cursor/mcp.json',
        '  <span class="badge-ok">✔</span> Gemini CLI   updated ~/.gemini/settings.json',
        '  <span class="badge-ok">✔</span> opencode     updated ~/.config/opencode/opencode.json',
    ]

    for i in range(1, len(apply_steps) + 1):
        step_body = applied_prompt + "\n" + "\n".join(apply_steps[:i])
        frames.append((step_body, 0.22))

    # Final completion message
    final_body = (
        applied_prompt + "\n" + "\n".join(apply_steps) + "\n\n"
        '  <span class="badge-ok">✔</span> quad successfully wired into your agents!\n\n'
        '  <span class="badge-step">›</span> Next steps:\n'
        '    1. Restart your agent (Claude Code, Codex, Cursor, Gemini CLI, or OpenCode).\n'
        '    2. Try asking: "what\'s due this week?" or "check my attendance".\n'
        '    3. Run `quad doctor` anytime to verify connection health.\n\n'
        '<div class="prompt-row"><span class="prompt-symbol">❯</span> <span class="prompt-path">~/oss/quad</span> <span class="cursor"></span></div>'
    )
    frames.append((final_body, 4.0))

    out_base = os.path.join(VIDEO_DIR, "quad-setup")
    build_video_from_frames(frames, out_base, "kushvinth@macbook: ~/oss/quad — quad setup", width=960, height=620)

def main():
    print("==> Building complete visual asset suite for quad...")
    generate_screenshots()
    generate_usage_video()
    generate_setup_video()
    print("==> All assets successfully generated!")

if __name__ == "__main__":
    main()
