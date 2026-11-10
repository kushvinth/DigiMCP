//! `quad login` — get a session token without making the user root around in
//! DevTools.
//!
//! The portal keeps the `user` cookie as the session token.
//! We provide:
//!   1. Automatic browser login: launches your browser directly to the portal,
//!      waits for you to sign in, intercepts the session token automatically
//!      over DevTools, and saves it. No manual DevTools copying needed!
//!   2. `--connect`: attaches to a browser you already opened with
//!      `--remote-debugging-port=9222`.
//!   3. `--token <jwt>`: paste a token directly.
//!   4. `--manual`: display the manual DevTools copy-paste instructions.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use crate::config;
use crate::error::{QuadError, jwt_claims};
use crate::portal::clean;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

struct BrowserGuard {
    child: Option<std::process::Child>,
    temp_dir: Option<PathBuf>,
}

impl Drop for BrowserGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(dir) = self.temp_dir.take() {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

pub fn run(args: Vec<String>) -> Result<(), BoxError> {
    let mut token_flag: Option<String> = None;
    let mut portal_flag: Option<String> = None;
    let mut connect = false;
    let mut manual = false;

    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--token" | "-t" => token_flag = it.next(),
            "--portal" | "-p" => portal_flag = it.next(),
            "--connect" => connect = true,
            "--manual" | "-m" => manual = true,
            "--help" | "-h" => {
                println!(
                    "usage: quad login [--portal <host>] [--token <jwt>] [--manual] [--connect]\n\
                     \n  --portal   your portal host, e.g. snuc.digiicampus.com\n\
                     \n  --token    paste the token directly instead of browser login\n\
                     \n  --manual   show manual DevTools copy-paste instructions\n\
                     \n  --connect  read token from a browser started with --remote-debugging-port=9222"
                );
                return Ok(());
            }
            other => {
                eprintln!("quad login: unknown option {other:?}");
                std::process::exit(2);
            }
        }
    }

    let portal = portal_flag.or_else(|| std::env::var("QUAD_BASE_URL").ok());

    let token = if let Some(t) = token_flag {
        t.trim().to_string()
    } else if manual {
        print_instructions(portal.as_deref());
        println!("\nPaste the token here and press enter (or ctrl-c to cancel):");
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        line.trim().to_string()
    } else if connect {
        read_from_devtools(portal.as_deref())?
    } else {
        auto_login_flow(portal.as_deref())?
    };

    if token.is_empty() {
        return Err("no token provided".into());
    }

    let claims = jwt_claims(&token).map_err(|e| QuadError::Config(e.to_string()))?;
    let from_token = clean(claims.get("instituteUrl"));
    let base_url = portal
        .map(|p| {
            if p.starts_with("http") {
                p
            } else {
                format!("https://{p}")
            }
        })
        .filter(|p| !p.trim().is_empty())
        .or(Some(from_token.clone()).filter(|s| !s.is_empty()))
        .ok_or_else(|| {
            QuadError::Config(
                "cannot tell which portal this token belongs to — pass --portal <host>".into(),
            )
        })?;

    let path = match config::save_token(&token, base_url.trim_end_matches('/')) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("quad login: could not save the token ({e}); export QUAD_TOKEN to use it.");
            return Ok(());
        }
    };

    let name = clean(claims.get("sub"));
    let exp = claims.get("exp").and_then(|v| v.as_i64());
    println!("✓ saved to {}", path.display());
    println!("  portal: {base_url}");
    if !name.is_empty() {
        println!("  token subject: {name}");
    }
    if let Some(exp) = exp {
        let days = (exp - config::now_unix()).div_euclid(86_400);
        println!("  expires in {days} day(s)");
    }
    println!("\nNext:  quad doctor   (verify)   ·   quad setup   (wire up your agents)");
    Ok(())
}

fn auto_login_flow(portal: Option<&str>) -> Result<String, BoxError> {
    let host = portal.unwrap_or("snuc.digiicampus.com");
    let portal_url = if host.starts_with("http") {
        host.to_string()
    } else {
        format!("https://{host}")
    };

    let mut _guard: Option<BrowserGuard> = None;

    // Check if port 9222 is already open
    let already_running = {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(500))
            .build()?;
        rt.block_on(async {
            client
                .get("http://127.0.0.1:9222/json")
                .send()
                .await
                .is_ok()
        })
    };

    if !already_running {
        if let Some(browser_bin) = find_browser() {
            let browser_name = browser_bin
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Browser");
            let temp_dir = std::env::temp_dir().join(format!("quad-login-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&temp_dir);

            println!("🌐 Launching {browser_name} at {portal_url}...");
            println!("🔑 Please sign in with your college credentials in the browser window.");
            print!("⏳ Waiting for sign-in (intercepting session cookie)...");
            let _ = std::io::stdout().flush();

            let child = std::process::Command::new(&browser_bin)
                .arg("--remote-debugging-port=9222")
                .arg(format!("--user-data-dir={}", temp_dir.display()))
                .arg("--no-first-run")
                .arg("--no-default-browser-check")
                .arg(&portal_url)
                .spawn();

            match child {
                Ok(c) => {
                    _guard = Some(BrowserGuard {
                        child: Some(c),
                        temp_dir: Some(temp_dir),
                    });
                }
                Err(e) => {
                    eprintln!("\nquad login: could not launch browser ({e})");
                }
            }
        } else {
            println!("• No supported Chromium browser found for auto-login.");
            print_instructions(portal);
            println!("\nPaste the token here and press enter (or ctrl-c to cancel):");
            let mut line = String::new();
            std::io::stdin().lock().read_line(&mut line)?;
            return Ok(line.trim().to_string());
        }
    } else {
        println!("🌐 Connected to browser on port 9222...");
        print!("⏳ Waiting for sign-in (checking for session cookie)...");
        let _ = std::io::stdout().flush();
    }

    match poll_cookie_from_devtools(180) {
        Ok(token) => {
            println!("✓ Sign-in detected! Captured session token.");
            Ok(token)
        }
        Err(e) => {
            eprintln!("\nquad login: {e}");
            print_instructions(portal);
            println!("\nPaste the token here and press enter (or ctrl-c to cancel):");
            let mut line = String::new();
            std::io::stdin().lock().read_line(&mut line)?;
            Ok(line.trim().to_string())
        }
    }
}

fn poll_cookie_from_devtools(timeout_secs: u64) -> Result<String, BoxError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()?;

    let start = std::time::Instant::now();
    let mut dots = 0;

    while start.elapsed().as_secs() < timeout_secs {
        let pages_res = runtime.block_on(async {
            client
                .get("http://127.0.0.1:9222/json")
                .send()
                .await?
                .json::<serde_json::Value>()
                .await
        });

        if let Some(cookie) = pages_res.ok().as_ref().and_then(try_extract_cookie) {
            println!();
            return Ok(cookie);
        }

        print!(".");
        let _ = std::io::stdout().flush();
        dots += 1;
        if dots >= 60 {
            println!();
            dots = 0;
        }
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }

    println!();
    Err(
        "timed out waiting for sign-in. Run with --token <jwt> to paste your token, or try again."
            .into(),
    )
}

fn try_extract_cookie(pages: &serde_json::Value) -> Option<String> {
    let list = pages.as_array()?;
    let page = list
        .iter()
        .find(|p| p.get("type").and_then(|t| t.as_str()) == Some("page"))
        .or_else(|| list.first())?;

    let ws_url = page.get("webSocketDebuggerUrl")?.as_str()?;
    let host = ws_url.trim_start_matches("ws://").split('/').next()?;
    let addr: std::net::SocketAddr = host.parse().ok()?;
    let mut stream =
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(2)).ok()?;

    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .ok()?;
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(2)))
        .ok()?;

    let key = "dGhlIHNhbXBsZSBub25jZQ==";
    let path = ws_url
        .trim_start_matches("ws://")
        .split_once('/')
        .map(|(_, p)| format!("/{p}"))
        .unwrap_or_else(|| "/".into());

    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).ok()?;

    let mut reader = std::io::BufReader::new(stream.try_clone().ok()?);
    let mut head = String::new();
    for _ in 0..20 {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        head.push_str(&line);
        if line.trim().is_empty() {
            break;
        }
    }
    if !head.starts_with("HTTP/1.1 101") {
        return None;
    }

    let msg = serde_json::json!({
        "id": 1,
        "method": "Network.getAllCookies",
        "params": {}
    });
    ws_send(stream.try_clone().ok()?, serde_json::to_string(&msg).ok()?).ok()?;

    let frame = ws_read(&mut reader).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&frame).ok()?;
    let cookie = parsed
        .get("result")
        .and_then(|r| r.get("cookies"))
        .and_then(|c| c.as_array())
        .and_then(|list| {
            list.iter().find(|c| {
                let name = c.get("name").and_then(|n| n.as_str());
                let val = c.get("value").and_then(|v| v.as_str()).unwrap_or("");
                name == Some("user") && val.starts_with("eyJ")
            })
        })
        .and_then(|c| c.get("value").and_then(|v| v.as_str()))?;

    Some(cookie.to_string())
}

fn find_browser() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let candidates = [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Arc.app/Contents/MacOS/Arc",
        ];
        for c in candidates {
            let p = PathBuf::from(c);
            if p.exists() {
                return Some(p);
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        let candidates = [
            "google-chrome-stable",
            "google-chrome",
            "chromium",
            "chromium-browser",
            "brave-browser",
        ];
        for c in candidates {
            if let Ok(output) = std::process::Command::new("which").arg(c).output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        return Some(PathBuf::from(path));
                    }
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        let candidates = [
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        ];
        for c in candidates {
            let p = PathBuf::from(c);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

pub fn logout() -> Result<(), BoxError> {
    let Some(dir) = config::config_dir() else {
        return Ok(());
    };
    let path = dir.join("config.toml");
    match std::fs::remove_file(&path) {
        Ok(()) => println!("✓ removed {}", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("• nothing saved at {}", path.display())
        }
        Err(e) => return Err(format!("could not remove {}: {e}", path.display()).into()),
    }
    Ok(())
}

fn print_instructions(portal: Option<&str>) {
    let host = portal.unwrap_or("<your-institute>.digiicampus.com");
    let host = host.trim_start_matches("https://").trim_end_matches('/');
    println!(
        "To get your session token manually:\n\
         \n  1. Open https://{host} and sign in.\n\
         \n  2. Open DevTools (⌥⌘I on macOS, F12 on Windows/Linux) → Application → Cookies → https://{host}\n\
         \n  3. Find the cookie named `user` and copy its value (it starts with `eyJ`).\n\
         \n  4. Run:  quad login --token <paste-value-here>\n\
         \n  Or run `quad login --portal {host}` to have quad open your browser and capture it automatically."
    );
}

/// Read the `user` cookie from a Chromium browser's DevTools endpoint.
fn read_from_devtools(_portal: Option<&str>) -> Result<String, BoxError> {
    poll_cookie_from_devtools(15)
}

fn ws_send(mut stream: std::net::TcpStream, payload: String) -> std::io::Result<()> {
    let bytes = payload.as_bytes();
    let mut frame = vec![0x81u8];
    let mask = [0x1Au8, 0x2Bu8, 0x3Cu8, 0x4Du8];
    match bytes.len() {
        n if n < 126 => frame.push(0x80 | n as u8),
        n if n < 65_536 => {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(&mask);
    frame.extend(bytes.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    stream.write_all(&frame)
}

fn ws_read(reader: &mut std::io::BufReader<std::net::TcpStream>) -> std::io::Result<String> {
    loop {
        let mut header = [0u8; 2];
        std::io::Read::read_exact(reader, &mut header)?;
        let opcode = header[0] & 0x0F;
        let mut len = (header[1] & 0x7F) as usize;
        if len == 126 {
            let mut ext = [0u8; 2];
            std::io::Read::read_exact(reader, &mut ext)?;
            len = u16::from_be_bytes(ext) as usize;
        } else if len == 127 {
            let mut ext = [0u8; 8];
            std::io::Read::read_exact(reader, &mut ext)?;
            len = u64::from_be_bytes(ext) as usize;
        }
        let mut data = vec![0u8; len];
        std::io::Read::read_exact(reader, &mut data)?;

        if opcode == 1 {
            let s = String::from_utf8_lossy(&data).into_owned();
            if s.contains("\"id\":1") || s.contains("\"cookies\"") || s.contains("\"result\"") {
                return Ok(s);
            }
        } else if opcode == 8 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "websocket closed",
            ));
        }
    }
}
