//! `quad login` — get a session token without making the user root around in
//! DevTools.
//!
//! Directly extracts the session token (`Auth-Token` or `user` JWT) from installed
//! browsers on disk (Zen, Firefox, Chrome, Brave, Arc, Edge, etc.) without any
//! heavyweight browser wrappers or remote debugging daemons.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use crate::config;
use crate::error::{QuadError, jwt_claims};
use crate::portal::clean;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

struct BrowserCookieSource {
    browser_name: &'static str,
    db_paths: Vec<PathBuf>,
    is_chromium: bool,
    #[cfg(target_os = "macos")]
    keychain_service: Option<&'static str>,
}

struct FoundSession {
    token: String,
    browser: &'static str,
    exp: i64,
}

pub fn run(args: Vec<String>) -> Result<(), BoxError> {
    let mut token_flag: Option<String> = None;
    let mut portal_flag: Option<String> = None;
    let mut manual = false;

    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--token" | "-t" => token_flag = it.next(),
            "--portal" | "-p" => portal_flag = it.next(),
            "--manual" | "-m" => manual = true,
            "--help" | "-h" => {
                println!(
                    "usage: quad login [--portal <host>] [--token <jwt>] [--manual]\n\
                     \n  --portal   your portal host, e.g. snuc.digiicampus.com\n\
                     \n  --token    paste the token directly\n\
                     \n  --manual   show manual DevTools copy-paste instructions"
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
    } else {
        // Fast direct pull from installed browsers on disk (Zen, Firefox, Chrome, Brave, Arc, Edge, etc.)
        let (active, expired) = scan_all_browsers();
        if let Some(session) = active {
            let days = (session.exp - config::now_unix()).div_euclid(86_400);
            println!(
                "✓ Found active session in {}! (expires in {} days)",
                session.browser, days
            );
            session.token
        } else {
            let host = portal.as_deref().unwrap_or("snuc.digiicampus.com");
            let portal_url = if host.starts_with("http") {
                host.to_string()
            } else {
                format!("https://{host}")
            };

            if let Some(exp) = expired {
                let days = (config::now_unix() - exp.exp).div_euclid(86_400);
                println!(
                    "• Found session in {}, but it expired {} day(s) ago.",
                    exp.browser, days
                );
            } else {
                println!("• No active session found in installed browsers.");
            }

            println!("🌐 Opening {portal_url} in your default browser...");
            if let Err(e) = open_default_browser(&portal_url) {
                eprintln!("• Could not automatically open default browser: {e}");
            }
            println!("🔑 Please sign in with your college account in the browser.");
            print!("⏳ Waiting for sign-in (auto-detecting token from your browser)...");
            let _ = std::io::stdout().flush();

            let start = std::time::Instant::now();
            let timeout_secs = 180;
            let mut captured = None;
            let mut dots = 0;

            while start.elapsed().as_secs() < timeout_secs {
                let (active, _) = scan_all_browsers();
                if let Some(session) = active {
                    println!(
                        "\n✓ Sign-in detected from {}! Captured session token.",
                        session.browser
                    );
                    captured = Some(session.token);
                    break;
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

            match captured {
                Some(t) => t,
                None => {
                    println!();
                    eprintln!("quad login: timed out waiting for sign-in.");
                    print_instructions(portal.as_deref());
                    println!("\nPaste the token here and press enter (or ctrl-c to cancel):");
                    let mut line = String::new();
                    std::io::stdin().lock().read_line(&mut line)?;
                    line.trim().to_string()
                }
            }
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

fn scan_all_browsers() -> (Option<FoundSession>, Option<FoundSession>) {
    let unexpired = None;
    let mut expired = None;
    let now = config::now_unix();

    for src in get_all_browser_sources() {
        for db in &src.db_paths {
            let tokens = if src.is_chromium {
                #[cfg(target_os = "macos")]
                {
                    extract_from_chromium(db, src.keychain_service)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    extract_from_chromium(db, None)
                }
            } else {
                extract_from_gecko(db)
            };

            for token in tokens {
                if let Ok(claims) = jwt_claims(&token)
                    && let Some(exp) = claims.get("exp").and_then(|v| v.as_i64())
                {
                    if exp > now {
                        return (
                            Some(FoundSession {
                                token,
                                browser: src.browser_name,
                                exp,
                            }),
                            None,
                        );
                    } else if expired.is_none() {
                        expired = Some(FoundSession {
                            token,
                            browser: src.browser_name,
                            exp,
                        });
                    }
                }
            }
        }
    }

    (unexpired, expired)
}

fn get_all_browser_sources() -> Vec<BrowserCookieSource> {
    let mut sources = Vec::new();
    let home = match std::env::var_os("HOME") {
        Some(h) => PathBuf::from(h),
        None => return sources,
    };

    #[cfg(target_os = "macos")]
    let app_sup = home.join("Library/Application Support");

    // 1. Gecko-based browsers (Firefox, Zen, LibreWolf, Floorp, Waterfox)
    #[cfg(target_os = "macos")]
    let gecko_specs = [
        ("Zen Browser", app_sup.join("zen/Profiles")),
        ("Firefox", app_sup.join("Firefox/Profiles")),
        ("LibreWolf", app_sup.join("librewolf/Profiles")),
        ("Floorp", app_sup.join("Floorp/Profiles")),
        ("Waterfox", app_sup.join("Waterfox/Profiles")),
    ];

    #[cfg(target_os = "linux")]
    let gecko_specs = [
        ("Zen Browser", home.join(".zen")),
        ("Firefox", home.join(".mozilla/firefox")),
        ("LibreWolf", home.join(".librewolf")),
        ("Floorp", home.join(".floorp")),
        ("Waterfox", home.join(".waterfox")),
    ];

    #[cfg(target_os = "windows")]
    let app_data = home.join("AppData/Roaming");
    #[cfg(target_os = "windows")]
    let gecko_specs = [
        ("Zen Browser", app_data.join("zen/Profiles")),
        ("Firefox", app_data.join("Mozilla/Firefox/Profiles")),
        ("LibreWolf", app_data.join("librewolf/Profiles")),
        ("Floorp", app_data.join("Floorp/Profiles")),
        ("Waterfox", app_data.join("Waterfox/Profiles")),
    ];

    for (name, profiles_dir) in gecko_specs {
        let mut paths = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&profiles_dir) {
            for entry in entries.flatten() {
                let db = entry.path().join("cookies.sqlite");
                if db.exists() {
                    paths.push(db);
                }
            }
        }
        if !paths.is_empty() {
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
            sources.push(BrowserCookieSource {
                browser_name: name,
                db_paths: paths,
                is_chromium: false,
                #[cfg(target_os = "macos")]
                keychain_service: None,
            });
        }
    }

    // 2. Chromium-based browsers (Chrome, Brave, Arc, Edge, Chromium, Vivaldi, Opera)
    #[cfg(target_os = "macos")]
    let chromium_specs = [
        (
            "Google Chrome",
            app_sup.join("Google/Chrome"),
            "Chrome Safe Storage",
        ),
        (
            "Brave Browser",
            app_sup.join("BraveSoftware/Brave-Browser"),
            "Brave Safe Storage",
        ),
        ("Arc", app_sup.join("Arc/User Data"), "Arc Safe Storage"),
        (
            "Microsoft Edge",
            app_sup.join("Microsoft Edge"),
            "Microsoft Edge Safe Storage",
        ),
        (
            "Chromium",
            app_sup.join("Chromium"),
            "Chromium Safe Storage",
        ),
        ("Vivaldi", app_sup.join("Vivaldi"), "Vivaldi Safe Storage"),
        (
            "Opera",
            app_sup.join("com.operasoftware.Opera"),
            "Opera Safe Storage",
        ),
    ];

    #[cfg(target_os = "macos")]
    for (name, user_dir, service) in chromium_specs {
        let mut paths = Vec::new();
        let default_db = user_dir.join("Default/Cookies");
        if default_db.exists() {
            paths.push(default_db);
        }
        let root_db = user_dir.join("Cookies");
        if root_db.exists() {
            paths.push(root_db);
        }
        if let Ok(entries) = std::fs::read_dir(&user_dir) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let s = file_name.to_string_lossy();
                if s.starts_with("Profile ") {
                    let db = entry.path().join("Cookies");
                    if db.exists() {
                        paths.push(db);
                    }
                }
            }
        }
        if !paths.is_empty() {
            sources.push(BrowserCookieSource {
                browser_name: name,
                db_paths: paths,
                is_chromium: true,
                keychain_service: Some(service),
            });
        }
    }

    sources
}

fn extract_from_gecko(db_path: &Path) -> Vec<String> {
    let tmp_id = std::process::id();
    let tmp_db = std::env::temp_dir().join(format!("quad_gecko_{tmp_id}.sqlite"));
    let tmp_wal = std::env::temp_dir().join(format!("quad_gecko_{tmp_id}.sqlite-wal"));

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
        .output();

    let _ = std::fs::remove_file(&tmp_db);
    let _ = std::fs::remove_file(&tmp_wal);

    let mut out = Vec::new();
    if let Ok(res) = output
        && res.status.success()
    {
        let text = String::from_utf8_lossy(&res.stdout);
        for line in text.lines() {
            let val = line.trim();
            if val.starts_with("eyJ") {
                out.push(val.to_string());
            }
        }
    }
    out
}

fn extract_from_chromium(db_path: &Path, service: Option<&str>) -> Vec<String> {
    let tmp_id = std::process::id();
    let tmp_db = std::env::temp_dir().join(format!("quad_chrom_{tmp_id}.sqlite"));
    let tmp_wal = std::env::temp_dir().join(format!("quad_chrom_{tmp_id}.sqlite-wal"));

    let _ = std::fs::copy(db_path, &tmp_db);
    let wal_path = db_path.with_file_name("Cookies-wal");
    if wal_path.exists() {
        let _ = std::fs::copy(&wal_path, &tmp_wal);
    }

    let output = std::process::Command::new("sqlite3")
        .arg(&tmp_db)
        .arg("SELECT value, hex(encrypted_value) FROM cookies WHERE host_key LIKE '%digiicampus%' AND (name='Auth-Token' OR name='user') ORDER BY creation_utc DESC;")
        .arg("-noheader")
        .arg("-batch")
        .output();

    let _ = std::fs::remove_file(&tmp_db);
    let _ = std::fs::remove_file(&tmp_wal);

    let mut out = Vec::new();
    if let Ok(res) = output
        && res.status.success()
    {
        let text = String::from_utf8_lossy(&res.stdout);
        for line in text.lines() {
            let parts: Vec<&str> = line.split('|').map(str::trim).collect();
            if let Some(val) = parts.first()
                && val.starts_with("eyJ")
            {
                out.push(val.to_string());
                continue;
            }
            if let Some(hex_val) = parts.get(1)
                && !hex_val.is_empty()
            {
                #[cfg(target_os = "macos")]
                if let Some(service_name) = service
                    && let Some(decrypted) = decrypt_chromium_cookie_macos(service_name, hex_val)
                    && let Some(idx) = decrypted.find("eyJ")
                {
                    out.push(decrypted[idx..].to_string());
                }
            }
        }
    }
    out
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn CCKeyDerivationPBKDF(
        algorithm: u32,
        password: *const u8,
        password_len: usize,
        salt: *const u8,
        salt_len: usize,
        prf: u32,
        rounds: u32,
        derived_key: *mut u8,
        derived_key_len: usize,
    ) -> i32;

    fn CCCrypt(
        op: u32,
        alg: u32,
        options: u32,
        key: *const u8,
        key_len: usize,
        iv: *const u8,
        data_in: *const u8,
        data_in_len: usize,
        data_out: *mut u8,
        data_out_available: usize,
        data_out_moved: *mut usize,
    ) -> i32;
}

#[cfg(target_os = "macos")]
fn decrypt_chromium_cookie_macos(service: &str, raw_hex: &str) -> Option<String> {
    if raw_hex.len() < 6 {
        return None;
    }
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-w", "-s", service])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let password = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if password.is_empty() {
        return None;
    }

    let raw = (0..raw_hex.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&raw_hex[i..i + 2], 16).ok())
        .collect::<Vec<u8>>();

    if raw.len() <= 3 || &raw[..3] != b"v10" {
        return None;
    }
    let ciphertext = &raw[3..];

    let mut key = [0u8; 16];
    let salt = b"saltysalt";
    let res = unsafe {
        CCKeyDerivationPBKDF(
            2, // kCCPBKDF2
            password.as_ptr(),
            password.len(),
            salt.as_ptr(),
            salt.len(),
            1, // kCCPRFHmacAlgSHA1
            1003,
            key.as_mut_ptr(),
            key.len(),
        )
    };
    if res != 0 {
        return None;
    }

    let iv = [0x20u8; 16]; // 16 spaces
    let mut plain = vec![0u8; ciphertext.len() + 16];
    let mut moved = 0usize;

    let crypt_res = unsafe {
        CCCrypt(
            1, // kCCDecrypt
            0, // kCCAlgorithmAES
            1, // kCCOptionPKCS7Padding
            key.as_ptr(),
            key.len(),
            iv.as_ptr(),
            ciphertext.as_ptr(),
            ciphertext.len(),
            plain.as_mut_ptr(),
            plain.len(),
            &mut moved,
        )
    };
    if crypt_res != 0 {
        return None;
    }

    Some(String::from_utf8_lossy(&plain[..moved]).into_owned())
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
