//! `quad login` — get a session token without making the user root around in
//! DevTools.
//!
//! The portal keeps the `user` cookie as the session token. Three ways in, in
//! order of how much work they put on the student:
//!
//!   1. `--token <jwt>` — paste it, we validate and save it.
//!   2. devtools on port 9222 — `quad login --connect` attaches to a browser the
//!      student already opened with `--remote-debugging-port=9222`.
//!   3. no token at all — we tell them exactly where to click, then read the
//!      token from stdin (handy when an agent drives the session).

use std::io::{BufRead, Write};

use crate::config;
use crate::error::{QuadError, jwt_claims};
use crate::portal::clean;

pub fn run(args: Vec<String>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut token_flag: Option<String> = None;
    let mut portal_flag: Option<String> = None;
    let mut connect = false;

    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--token" | "-t" => token_flag = it.next(),
            "--portal" | "-p" => portal_flag = it.next(),
            "--connect" => connect = true,
            "--help" | "-h" => {
                println!(
                    "usage: quad login [--portal <host>] [--token <jwt>] [--connect]\n\
                     \n  --portal   your portal host, e.g. mycollege.digiicampus.com\n\
                     \n  --token    paste the token from the browser instead of a browser session\n\
                     \n  --connect  read the token from a browser started with --remote-debugging-port=9222"
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

    let token = match token_flag {
        Some(t) => t.trim().to_string(),
        None if connect => match read_from_devtools(portal.as_deref()) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("quad login: {e}");
                print_instructions(portal.as_deref());
                return Ok(());
            }
        },
        None => {
            print_instructions(portal.as_deref());
            println!("\nPaste the token here and press enter (or ctrl-c to cancel):");
            let mut line = String::new();
            std::io::stdin().lock().read_line(&mut line)?;
            line.trim().to_string()
        }
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
            // Still usable for this session via the environment.
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

pub fn logout() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
        "To get your session token, in five steps:\n\
         \n  1. Open https://{host} and sign in.\n\
         \n  2. Open DevTools (⌥⌘I on macOS, F12 on Windows/Linux) → Application → Cookies → https://{host}\n\
         \n  3. Find the cookie named `user` and copy its value (it starts with `eyJ`).\n\
         \n  4. Run:  quad login --portal {host}\n\
         \n  5. Paste when asked. Then run `quad doctor`.\n\
         \n  Shortcut: start Chrome/Edge with --remote-debugging-port=9222, sign in, then\n  run `quad login --connect` and quad will read the cookie for you."
    );
}

/// Read the `user` cookie from a Chromium browser's DevTools endpoint.
fn read_from_devtools(_portal: Option<&str>) -> Result<String, BoxError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let client = reqwest::Client::new();
        let pages: serde_json::Value = client
            .get("http://127.0.0.1:9222/json")
            .send()
            .await
            .map_err(|e| format!("no browser on port 9222 ({e}); start one with --remote-debugging-port=9222"))?
            .json()
            .await?;
        let ws_url = pages
            .as_array()
            .and_then(|list| {
                list.iter().find_map(|p| {
                    p.get("webSocketDebuggerUrl")
                        .and_then(|u| u.as_str())
                        .map(str::to_string)
                })
            })
            .ok_or("the browser has no debuggable page open")?;
        // DevTools' cookie API needs a websocket; the handshake is the only part
        // we need, and doing it by hand keeps the binary dependency-free.
        let host = ws_url
            .trim_start_matches("ws://")
            .split('/')
            .next()
            .ok_or("unexpected devtools url")?;
        let mut stream = std::net::TcpStream::connect(host)?;
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let req = format!(
            "GET {} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n",
            ws_url.trim_start_matches("ws://").split_once('/').map(|(_, p)| format!("/{p}")).unwrap_or_else(|| "/".into())
        );
        stream.write_all(req.as_bytes())?;
        let mut reader = std::io::BufReader::new(stream.try_clone()?);
        let mut head = String::new();
        for _ in 0..20 {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            head.push_str(&line);
            if line.trim().is_empty() {
                break;
            }
        }
        if !head.starts_with("HTTP/1.1 101") {
            return Err("browser refused the devtools websocket".into());
        }
        let msg = serde_json::json!({
            "id": 1,
            "method": "Network.getAllCookies",
            "params": {}
        });
        ws_send(stream.try_clone()?, serde_json::to_string(&msg)?)?;
        let frame = ws_read(&mut reader)?;
        let parsed: serde_json::Value = serde_json::from_str(&frame)?;
        let cookie = parsed
            .get("result")
            .and_then(|r| r.get("cookies"))
            .and_then(|c| c.as_array())
            .and_then(|list| {
                list.iter()
                    .find(|c| c.get("name").and_then(|n| n.as_str()) == Some("user"))
            })
            .and_then(|c| c.get("value").and_then(|v| v.as_str()))
            .ok_or("the browser has no `user` cookie — sign in to the portal first")?;
        Ok(cookie.to_string())
    })
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

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
    let mut header = [0u8; 2];
    std::io::Read::read_exact(reader, &mut header)?;
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
    Ok(String::from_utf8_lossy(&data).into_owned())
}
