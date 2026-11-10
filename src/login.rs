//! `quad login` — get a session token without making the user root around in
//! DevTools.
//!
//! The portal keeps the `Auth-Token` / `user` cookie as the session token.
//! We provide:
//!   1. Default browser login: opens the portal in your default browser,
//!      waits for you to sign in, intercepts the session token automatically
//!      from your browser profile (Zen, Firefox, Chrome, etc.), and saves it.
//!   2. `--connect`: attaches to a browser started with `--remote-debugging-port=9222`.
//!   3. `--token <jwt>`: paste a token directly.
//!   4. `--manual`: display the manual DevTools copy-paste instructions.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use crate::config;
use crate::error::{QuadError, jwt_claims};
use crate::portal::clean;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

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
        default_browser_login_flow(portal.as_deref())?
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
        if days >= 0 {
            println!("  expires in {days} day(s)");
        } else {
            println!("  expired {} day(s) ago — sign in again to refresh", -days);
        }
    }
    println!("\nNext:  quad doctor   (verify)   ·   quad setup   (wire up your agents)");
    Ok(())
}

fn default_browser_login_flow(portal: Option<&str>) -> Result<String, BoxError> {
    let host = portal.unwrap_or("snuc.digiicampus.com");
    let portal_url = if host.starts_with("http") {
        host.to_string()
    } else {
        format!("https://{host}")
    };

    // 1. Check if there is already an active (unexpired) session in a default/gecko browser
    if let Some(token) = find_unexpired_gecko_token(portal) {
        println!("✓ Found existing active session in your browser!");
        return Ok(token);
    }

    // 2. Open default browser to the portal
    println!("🌐 Opening {portal_url} in your default browser...");
    if let Err(e) = open_default_browser(&portal_url) {
        eprintln!("• Could not automatically open default browser: {e}");
    }

    println!("🔑 Please sign in with your college credentials in your browser.");
    print!("⏳ Waiting for sign-in (intercepting session cookie)...");
    let _ = std::io::stdout().flush();

    // 3. Poll for the session token in the browser's cookie database (or devtools if open)
    let start = std::time::Instant::now();
    let timeout_secs = 180;
    let mut dots = 0;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(800))
        .build()?;

    while start.elapsed().as_secs() < timeout_secs {
        // Check Gecko/Firefox/Zen cookies.sqlite
        if let Some(token) = find_unexpired_gecko_token(portal) {
            println!("\n✓ Sign-in detected in default browser!");
            return Ok(token);
        }

        // Check Chromium DevTools if port 9222 is open
        let pages_res = runtime.block_on(async {
            client
                .get("http://127.0.0.1:9222/json")
                .send()
                .await?
                .json::<serde_json::Value>()
                .await
        });
        if let Some(cookie) = pages_res.ok().as_ref().and_then(try_extract_cookie) {
            println!("\n✓ Sign-in detected via DevTools!");
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
    eprintln!("quad login: timed out waiting for sign-in.");
    print_instructions(portal);
    println!("\nPaste the token here and press enter (or ctrl-c to cancel):");
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn open_default_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", url])
            .spawn()?;
    }
    Ok(())
}

fn find_unexpired_gecko_token(_portal: Option<&str>) -> Option<String> {
    for db_path in gecko_cookie_paths() {
        if let Some(token) = extract_cookie_from_sqlite(&db_path)
            && is_valid_unexpired(&token)
        {
            return Some(token);
        }
    }
    None
}

fn is_valid_unexpired(token: &str) -> bool {
    if let Ok(claims) = jwt_claims(token)
        && let Some(exp) = claims.get("exp").and_then(|v| v.as_i64())
    {
        return exp > config::now_unix();
    }
    false
}

fn gecko_cookie_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let home = match std::env::var_os("HOME") {
        Some(h) => PathBuf::from(h),
        None => return paths,
    };

    #[cfg(target_os = "macos")]
    let base_dirs = [
        home.join("Library/Application Support/zen/Profiles"),
        home.join("Library/Application Support/Firefox/Profiles"),
        home.join("Library/Application Support/Floorp/Profiles"),
        home.join("Library/Application Support/librewolf/Profiles"),
        home.join("Library/Application Support/Waterfox/Profiles"),
    ];

    #[cfg(target_os = "linux")]
    let base_dirs = [
        home.join(".zen"),
        home.join(".mozilla/firefox"),
        home.join(".floorp"),
        home.join(".librewolf"),
        home.join(".waterfox"),
    ];

    #[cfg(target_os = "windows")]
    let base_dirs = [
        home.join("AppData/Roaming/zen/Profiles"),
        home.join("AppData/Roaming/Mozilla/Firefox/Profiles"),
    ];

    for base in base_dirs {
        if let Ok(entries) = std::fs::read_dir(&base) {
            for entry in entries.flatten() {
                let db = entry.path().join("cookies.sqlite");
                if db.exists() {
                    paths.push(db);
                }
            }
        }
    }

    paths.sort_by(|a, b| {
        let ma = a
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let mb = b
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        mb.cmp(&ma)
    });

    paths
}

fn extract_cookie_from_sqlite(db_path: &Path) -> Option<String> {
    let tmp_id = std::process::id();
    let tmp_db = std::env::temp_dir().join(format!("quad_tmp_{tmp_id}.sqlite"));
    let tmp_wal = std::env::temp_dir().join(format!("quad_tmp_{tmp_id}.sqlite-wal"));

    let _ = std::fs::copy(db_path, &tmp_db);
    let wal_path = db_path.with_file_name("cookies.sqlite-wal");
    if wal_path.exists() {
        let _ = std::fs::copy(&wal_path, &tmp_wal);
    }

    let output = std::process::Command::new("sqlite3")
        .arg(&tmp_db)
        .arg("SELECT value FROM moz_cookies WHERE host LIKE '%digiicampus%' AND (name='Auth-Token' OR name='user') ORDER BY lastAccessed DESC;")
        .arg("-noheader")
        .arg("-batch")
        .output()
        .ok()?;

    let _ = std::fs::remove_file(&tmp_db);
    let _ = std::fs::remove_file(&tmp_wal);

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let val = line.trim();
        if val.starts_with("eyJ") {
            return Some(val.to_string());
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
         \n  3. Find the cookie named `user` or `Auth-Token` and copy its value (starts with `eyJ`).\n\
         \n  4. Run:  quad login --token <paste-value-here>"
    );
}

/// Read the `user` cookie from a Chromium browser's DevTools endpoint.
fn read_from_devtools(_portal: Option<&str>) -> Result<String, BoxError> {
    poll_cookie_from_devtools(15)
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
                (name == Some("user") || name == Some("Auth-Token")) && val.starts_with("eyJ")
            })
        })
        .and_then(|c| c.get("value").and_then(|v| v.as_str()))?;

    Some(cookie.to_string())
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
