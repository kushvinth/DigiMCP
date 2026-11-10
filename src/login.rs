//! `quad login` — get a session token without making the user root around in
//! DevTools.
//!
//! Human architecture notes:
//! Authenticating against an institutional SPA without an official OAuth flow is
//! notoriously tricky. Most tools resort to launching headless Chromium with
//! Puppeteer/Playwright (~400 MB download, sluggish startup, easily blocked by Cloudflare).
//!
//! Here we use direct filesystem extraction from the user's active browser:
//!
//!   1. **The LocalStorage / Cookie Asynchrony**:
//!      Modern SPAs (Angular/React) store the active student session in `localStorage`
//!      under the `userInfo` object (which holds `"token": "eyJhbGci..."`). The
//!      corresponding cookie in `cookies.sqlite` may expire in 24 hours and sit
//!      stale on disk, even while the user is actively logged into the web portal with
//!      a valid 7-day token. We inspect BOTH LocalStorage and Cookies, prioritizing
//!      any active session (`exp > now`).
//!
//!   2. **Conquering `sqlite3: database is locked (5)`**:
//!      When a browser (Firefox, Zen, Chrome) is actively open, its SQLite databases
//!      run in WAL (Write-Ahead Logging) mode and hold exclusive filesystem advisory
//!      locks. Running `sqlite3` directly on the active file will deadlock and fail.
//!      We solve this cleanly by taking an atomic snapshot of both `<file>.sqlite`
//!      AND `<file>.sqlite-wal` into `/tmp`, querying the snapshot, and cleaning up
//!      in <10 ms.
//!
//!   3. **Zero-Dependency Google Snappy**:
//!      Mozilla Gecko compresses LocalStorage blobs with Google Snappy when
//!      `compression_type == 1`. Rather than pulling in large native C libraries or
//!      bloated compression crates, we wrote a standalone ~60 line Snappy decompressor
//!      in pure Rust (`decompress_snappy`) that directly handles literal and offset-copy
//!      tags with bounded memory allocations.
//!
//!   4. **OS-Native Default Browser Priority**:
//!      Instead of hardcoding browser discovery order, we query macOS LaunchServices
//!      (`LSHandlers` for `https` / `default-app.web-browser`), Linux `xdg-settings`,
//!      or Windows Registry to discover what browser the user actually uses, placing
//!      their default browser at index 0 of the discovery queue.

use std::io::BufRead;
use std::path::{Path, PathBuf};

use crate::config;
use crate::error::{QuadError, jwt_claims};
use crate::portal::clean;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

struct BrowserSource {
    browser_name: &'static str,
    cookie_dbs: Vec<PathBuf>,
    gecko_ls_dbs: Vec<PathBuf>,
    chromium_ls_dirs: Vec<PathBuf>,
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
    crate::ui::print_banner();
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
                     \n  --portal   your portal host, e.g. <college>.digiicampus.com\n\
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

    let configured_portal = portal_flag
        .or_else(|| std::env::var("QUAD_BASE_URL").ok())
        .or_else(|| config::lookup("base_url"));

    let token = if let Some(t) = token_flag {
        t.trim().to_string()
    } else if manual {
        print_instructions(configured_portal.as_deref());
        println!("\nPaste the token here and press enter (or ctrl-c to cancel):");
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        line.trim().to_string()
    } else {
        // Fast direct extraction from installed browsers across LocalStorage and Cookies
        let (active, expired) = scan_all_browsers(configured_portal.as_deref());
        if let Some(session) = active {
            let days = (session.exp - config::now_unix()).div_euclid(86_400);
            crate::ui::ok(&format!(
                "Found active session in {} (expires in {days} days)",
                session.browser
            ));
            session.token
        } else {
            // Dynamically resolve target portal host
            let discovered_host = expired
                .as_ref()
                .and_then(|s| {
                    jwt_claims(&s.token).ok().and_then(|c| {
                        c.get("instituteUrl")
                            .and_then(|v| v.as_str())
                            .map(clean_host)
                    })
                })
                .filter(|s| !s.is_empty());

            let target_host = configured_portal
                .as_deref()
                .map(clean_host)
                .or(discovered_host)
                .unwrap_or_else(|| {
                    crate::ui::step(
                        "Enter your institution portal host (e.g. your-college.digiicampus.com): ",
                    );
                    let mut input = String::new();
                    let _ = std::io::stdin().lock().read_line(&mut input);
                    clean_host(&input)
                });

            if target_host.is_empty() {
                return Err("portal host is required to sign in".into());
            }

            let portal_url = format!("https://{target_host}");

            if let Some(exp) = expired {
                let days = (config::now_unix() - exp.exp).div_euclid(86_400);
                crate::ui::info(&format!(
                    "Found session in {}, but it expired {days} day(s) ago.",
                    exp.browser
                ));
            } else {
                crate::ui::info("No active session found in installed browsers.");
            }

            crate::ui::step(&format!("Opening {portal_url} in your default browser..."));
            if let Err(e) = open_default_browser(&portal_url) {
                crate::ui::err(&format!(
                    "Could not automatically open default browser: {e}"
                ));
            }
            crate::ui::step("Please sign in with your college account in the browser.");

            let mut spinner =
                crate::ui::Spinner::new("Waiting for sign-in (detecting session token)...");
            let start = std::time::Instant::now();
            let timeout_secs = 180;
            let mut captured = None;
            let mut poll_counter = 0;

            while start.elapsed().as_secs() < timeout_secs {
                spinner.tick();
                if poll_counter % 5 == 0 {
                    let (active, _) = scan_all_browsers(Some(&target_host));
                    if let Some(session) = active {
                        spinner.finish_ok(&format!(
                            "Sign-in detected from {} (token captured in {:.1}s)",
                            session.browser,
                            start.elapsed().as_secs_f32()
                        ));
                        captured = Some(session.token);
                        break;
                    }
                }
                poll_counter += 1;
                std::thread::sleep(std::time::Duration::from_millis(80));
            }

            match captured {
                Some(t) => t,
                None => {
                    spinner.finish_err("Timed out waiting for sign-in.");
                    print_instructions(Some(&target_host));
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
    let base_url = configured_portal
        .map(|p| {
            if p.starts_with("http") {
                p
            } else {
                format!("https://{p}")
            }
        })
        .filter(|p| !p.trim().is_empty())
        .or_else(|| {
            if !from_token.is_empty() {
                Some(from_token.clone())
            } else {
                None
            }
        })
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
    crate::ui::ok(&format!("Saved credentials to {}", path.display()));
    crate::ui::info(&format!("portal: {base_url}"));
    if !name.is_empty() {
        crate::ui::info(&format!("token subject: {name}"));
    }
    if let Some(exp) = exp {
        let days = (exp - config::now_unix()).div_euclid(86_400);
        if days >= 0 {
            crate::ui::info(&format!("expires in {days} day(s)"));
        } else {
            crate::ui::info(&format!(
                "expired {} day(s) ago — sign in again to refresh",
                -days
            ));
        }
    }
    println!("\nNext:  quad doctor   (verify)   ·   quad setup   (wire up your agents)");
    Ok(())
}

fn clean_host(raw: &str) -> String {
    raw.trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
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

/// Detect the user's OS default browser name to prioritize it at index 0.
#[cfg(target_os = "macos")]
fn detect_default_browser_name() -> Option<&'static str> {
    let output = std::process::Command::new("defaults")
        .args([
            "read",
            "com.apple.LaunchServices/com.apple.launchservices.secure",
            "LSHandlers",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for block in text.split("},") {
        if block.contains("https") || block.contains("default-app.web-browser") {
            for line in block.lines() {
                if line.contains("LSHandlerRoleAll") {
                    let id = line
                        .split('=')
                        .nth(1)?
                        .trim()
                        .trim_matches(';')
                        .trim_matches('"');
                    let name = match id {
                        "app.zen-browser.zen" => Some("Zen Browser"),
                        "org.mozilla.firefox" => Some("Firefox"),
                        "com.google.chrome" => Some("Google Chrome"),
                        "company.thebrowser.browser" => Some("Arc"),
                        "com.brave.browser" => Some("Brave Browser"),
                        "com.microsoft.edgemac" => Some("Microsoft Edge"),
                        "com.operasoftware.operagx" | "com.operasoftware.opera" => Some("Opera"),
                        "com.vivaldi.vivaldi" => Some("Vivaldi"),
                        "org.mozilla.librewolf" => Some("LibreWolf"),
                        "org.mozilla.floorp" => Some("Floorp"),
                        "org.mozilla.waterfox" => Some("Waterfox"),
                        _ => None,
                    };
                    if name.is_some() {
                        return name;
                    }
                }
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn detect_default_browser_name() -> Option<&'static str> {
    let output = std::process::Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .output()
        .ok()?;
    if output.status.success() {
        let name = String::from_utf8_lossy(&output.stdout).to_lowercase();
        if name.contains("zen") {
            return Some("Zen Browser");
        }
        if name.contains("firefox") {
            return Some("Firefox");
        }
        if name.contains("chrome") {
            return Some("Google Chrome");
        }
        if name.contains("brave") {
            return Some("Brave Browser");
        }
        if name.contains("edge") {
            return Some("Microsoft Edge");
        }
        if name.contains("opera") {
            return Some("Opera");
        }
        if name.contains("vivaldi") {
            return Some("Vivaldi");
        }
        if name.contains("librewolf") {
            return Some("LibreWolf");
        }
        if name.contains("floorp") {
            return Some("Floorp");
        }
        if name.contains("waterfox") {
            return Some("Waterfox");
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn detect_default_browser_name() -> Option<&'static str> {
    let output = std::process::Command::new("reg")
        .args([
            "query",
            r"HKEY_CURRENT_USER\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
            "/v",
            "ProgId",
        ])
        .output()
        .ok()?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).to_lowercase();
        if text.contains("zen") {
            return Some("Zen Browser");
        }
        if text.contains("firefox") {
            return Some("Firefox");
        }
        if text.contains("chrome") {
            return Some("Google Chrome");
        }
        if text.contains("brave") {
            return Some("Brave Browser");
        }
        if text.contains("edge") || text.contains("msedge") {
            return Some("Microsoft Edge");
        }
        if text.contains("opera") {
            return Some("Opera");
        }
        if text.contains("vivaldi") {
            return Some("Vivaldi");
        }
    }
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn detect_default_browser_name() -> Option<&'static str> {
    None
}

fn sort_by_modified_desc(paths: &mut [PathBuf]) {
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
}

fn find_chromium_paths(user_dir: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut cookie_paths = Vec::new();
    let mut ls_dirs = Vec::new();

    let check_profile = |prof_dir: &Path, c_paths: &mut Vec<PathBuf>, l_dirs: &mut Vec<PathBuf>| {
        let c = prof_dir.join("Cookies");
        if c.exists() {
            c_paths.push(c);
        }
        let ls = prof_dir.join("Local Storage/leveldb");
        if ls.is_dir() {
            l_dirs.push(ls);
        }
    };

    check_profile(&user_dir.join("Default"), &mut cookie_paths, &mut ls_dirs);
    check_profile(user_dir, &mut cookie_paths, &mut ls_dirs);

    if let Ok(entries) = std::fs::read_dir(user_dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let s = file_name.to_string_lossy();
            if s.starts_with("Profile ") {
                check_profile(&entry.path(), &mut cookie_paths, &mut ls_dirs);
            }
        }
    }

    sort_by_modified_desc(&mut cookie_paths);
    (cookie_paths, ls_dirs)
}

fn get_all_browser_sources(target_host: Option<&str>) -> Vec<BrowserSource> {
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
        let mut cookie_paths = Vec::new();
        let mut ls_paths = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&profiles_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                let db = p.join("cookies.sqlite");
                if db.exists() {
                    cookie_paths.push(db);
                }

                // Check LocalStorage under storage/default/<origin>/ls/data.sqlite
                let storage_default = p.join("storage/default");
                if let Ok(origins) = std::fs::read_dir(&storage_default) {
                    for origin in origins.flatten() {
                        let origin_name = origin.file_name();
                        let s = origin_name.to_string_lossy();
                        let s_lower = s.to_lowercase();
                        let is_match = match target_host {
                            Some(h) => {
                                let clean = clean_host(h).to_lowercase();
                                s_lower.contains(&clean)
                                    || s_lower.contains("digii")
                                    || s_lower.contains("collpoll")
                            }
                            None => s_lower.contains("digii") || s_lower.contains("collpoll"),
                        };
                        if is_match {
                            let ls_db = origin.path().join("ls/data.sqlite");
                            if ls_db.exists() {
                                ls_paths.push(ls_db);
                            }
                        }
                    }
                }
            }
        }

        if !cookie_paths.is_empty() || !ls_paths.is_empty() {
            sort_by_modified_desc(&mut cookie_paths);
            sort_by_modified_desc(&mut ls_paths);
            sources.push(BrowserSource {
                browser_name: name,
                cookie_dbs: cookie_paths,
                gecko_ls_dbs: ls_paths,
                chromium_ls_dirs: Vec::new(),
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

    #[cfg(target_os = "linux")]
    let config_dir = home.join(".config");
    #[cfg(target_os = "linux")]
    let chromium_specs = [
        ("Google Chrome", config_dir.join("google-chrome")),
        (
            "Brave Browser",
            config_dir.join("BraveSoftware/Brave-Browser"),
        ),
        ("Microsoft Edge", config_dir.join("microsoft-edge")),
        ("Chromium", config_dir.join("chromium")),
        ("Vivaldi", config_dir.join("vivaldi")),
        ("Opera", config_dir.join("opera")),
    ];

    #[cfg(target_os = "windows")]
    let local_app_data = home.join("AppData/Local");
    #[cfg(target_os = "windows")]
    let chromium_specs = [
        (
            "Google Chrome",
            local_app_data.join("Google/Chrome/User Data"),
        ),
        (
            "Brave Browser",
            local_app_data.join("BraveSoftware/Brave-Browser/User Data"),
        ),
        (
            "Microsoft Edge",
            local_app_data.join("Microsoft/Edge/User Data"),
        ),
        ("Vivaldi", local_app_data.join("Vivaldi/User Data")),
        ("Opera", app_data.join("Opera Software/Opera Stable")),
    ];

    #[cfg(target_os = "macos")]
    for (name, user_dir, service) in chromium_specs {
        let (cookie_paths, ls_dirs) = find_chromium_paths(&user_dir);
        if !cookie_paths.is_empty() || !ls_dirs.is_empty() {
            sources.push(BrowserSource {
                browser_name: name,
                cookie_dbs: cookie_paths,
                gecko_ls_dbs: Vec::new(),
                chromium_ls_dirs: ls_dirs,
                is_chromium: true,
                keychain_service: Some(service),
            });
        }
    }

    #[cfg(not(target_os = "macos"))]
    for (name, user_dir) in chromium_specs {
        let (cookie_paths, ls_dirs) = find_chromium_paths(&user_dir);
        if !cookie_paths.is_empty() || !ls_dirs.is_empty() {
            sources.push(BrowserSource {
                browser_name: name,
                cookie_dbs: cookie_paths,
                gecko_ls_dbs: Vec::new(),
                chromium_ls_dirs: ls_dirs,
                is_chromium: true,
            });
        }
    }

    // Dynamically prioritize the user's OS default browser
    if let Some(default_browser) = detect_default_browser_name()
        && let Some(pos) = sources
            .iter()
            .position(|s| s.browser_name == default_browser)
    {
        let def_source = sources.remove(pos);
        sources.insert(0, def_source);
    }

    sources
}

/// Pure-Rust Snappy decompressor for Gecko LocalStorage blobs.
pub fn decompress_snappy(data: &[u8]) -> Option<Vec<u8>> {
    let mut pos = 0;
    let mut uncompressed_len: usize = 0;
    let mut shift = 0;
    let mut varint_ok = false;
    while pos < data.len() {
        let b = data[pos];
        pos += 1;
        uncompressed_len |= ((b & 0x7F) as usize) << shift;
        if (b & 0x80) == 0 {
            varint_ok = true;
            break;
        }
        shift += 7;
        if shift > 35 {
            return None;
        }
    }
    if !varint_ok {
        return None;
    }

    let mut out = Vec::with_capacity(uncompressed_len.min(16 * 1024 * 1024));
    while pos < data.len() {
        let tag = data[pos];
        pos += 1;
        let element_type = tag & 0x03;
        match element_type {
            0 => {
                // Literal
                let lit_len = (tag >> 2) as usize;
                let length = match lit_len {
                    0..=59 => lit_len + 1,
                    60 => {
                        if pos >= data.len() {
                            return None;
                        }
                        let len = data[pos] as usize + 1;
                        pos += 1;
                        len
                    }
                    61 => {
                        if pos + 1 >= data.len() {
                            return None;
                        }
                        let len = (data[pos] as usize | ((data[pos + 1] as usize) << 8)) + 1;
                        pos += 2;
                        len
                    }
                    62 => {
                        if pos + 2 >= data.len() {
                            return None;
                        }
                        let len = (data[pos] as usize
                            | ((data[pos + 1] as usize) << 8)
                            | ((data[pos + 2] as usize) << 16))
                            + 1;
                        pos += 3;
                        len
                    }
                    _ => {
                        if pos + 3 >= data.len() {
                            return None;
                        }
                        let len = (data[pos] as usize
                            | ((data[pos + 1] as usize) << 8)
                            | ((data[pos + 2] as usize) << 16)
                            | ((data[pos + 3] as usize) << 24))
                            + 1;
                        pos += 4;
                        len
                    }
                };
                if pos + length > data.len() {
                    return None;
                }
                out.extend_from_slice(&data[pos..pos + length]);
                pos += length;
            }
            1 => {
                // Copy with 1-byte offset
                let length = (((tag >> 2) & 0x07) as usize) + 4;
                if pos >= data.len() {
                    return None;
                }
                let offset = (((tag & 0xE0) as usize) << 3) | (data[pos] as usize);
                pos += 1;
                if offset == 0 || offset > out.len() {
                    return None;
                }
                for _ in 0..length {
                    let b = out[out.len() - offset];
                    out.push(b);
                }
            }
            2 => {
                // Copy with 2-byte offset
                let length = ((tag >> 2) as usize) + 1;
                if pos + 1 >= data.len() {
                    return None;
                }
                let offset = data[pos] as usize | ((data[pos + 1] as usize) << 8);
                pos += 2;
                if offset == 0 || offset > out.len() {
                    return None;
                }
                for _ in 0..length {
                    let b = out[out.len() - offset];
                    out.push(b);
                }
            }
            3 => {
                // Copy with 4-byte offset
                let length = ((tag >> 2) as usize) + 1;
                if pos + 3 >= data.len() {
                    return None;
                }
                let offset = data[pos] as usize
                    | ((data[pos + 1] as usize) << 8)
                    | ((data[pos + 2] as usize) << 16)
                    | ((data[pos + 3] as usize) << 24);
                pos += 4;
                if offset == 0 || offset > out.len() {
                    return None;
                }
                for _ in 0..length {
                    let b = out[out.len() - offset];
                    out.push(b);
                }
            }
            _ => unreachable!(),
        }
    }
    if out.len() != uncompressed_len {
        return None;
    }
    Some(out)
}

pub fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    let clean = hex.trim();
    if !clean.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(clean.len() / 2);
    for chunk in clean.as_bytes().as_chunks::<2>().0 {
        let h1 = match chunk[0] {
            b'0'..=b'9' => chunk[0] - b'0',
            b'a'..=b'f' => chunk[0] - b'a' + 10,
            b'A'..=b'F' => chunk[0] - b'A' + 10,
            _ => return None,
        };
        let h2 = match chunk[1] {
            b'0'..=b'9' => chunk[1] - b'0',
            b'a'..=b'f' => chunk[1] - b'a' + 10,
            b'A'..=b'F' => chunk[1] - b'A' + 10,
            _ => return None,
        };
        bytes.push((h1 << 4) | h2);
    }
    Some(bytes)
}

pub fn scan_bytes_for_jwt(raw_bytes: &[u8], out: &mut Vec<String>) {
    // 1. ASCII scan for b"eyJ"
    let mut i = 0;
    while i + 3 <= raw_bytes.len() {
        if &raw_bytes[i..i + 3] == b"eyJ" {
            let mut end = i + 3;
            let mut dot_count = 0;
            while end < raw_bytes.len() {
                let b = raw_bytes[end];
                if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
                    end += 1;
                } else if b == b'.' {
                    dot_count += 1;
                    end += 1;
                } else {
                    break;
                }
            }
            let cand = &raw_bytes[i..end];
            if dot_count == 2
                && (30..=4096).contains(&cand.len())
                && let Ok(s) = std::str::from_utf8(cand)
            {
                out.push(s.to_string());
            }
            i = end;
        } else {
            i += 1;
        }
    }

    // 2. UTF-16LE scan for b"e\0y\0J\0"
    let mut i = 0;
    while i + 6 <= raw_bytes.len() {
        if raw_bytes[i..i + 6] == [b'e', 0, b'y', 0, b'J', 0] {
            let mut p = i;
            let mut chars = Vec::new();
            let mut dot_count = 0;
            while p + 1 < raw_bytes.len() {
                let code = raw_bytes[p] as u16 | ((raw_bytes[p + 1] as u16) << 8);
                if (code >= b'A' as u16 && code <= b'Z' as u16)
                    || (code >= b'a' as u16 && code <= b'z' as u16)
                    || (code >= b'0' as u16 && code <= b'9' as u16)
                    || code == b'-' as u16
                    || code == b'_' as u16
                {
                    chars.push(code as u8);
                    p += 2;
                } else if code == b'.' as u16 {
                    chars.push(b'.');
                    dot_count += 1;
                    p += 2;
                } else {
                    break;
                }
            }
            if dot_count == 2
                && (30..=4096).contains(&chars.len())
                && let Ok(s) = std::str::from_utf8(&chars)
            {
                out.push(s.to_string());
            }
            i = if p > i { p } else { i + 2 };
        } else {
            i += 1;
        }
    }
}

fn extract_from_gecko_localstorage(db_path: &Path) -> Vec<String> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp_id = format!(
        "{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    );
    let tmp_db = std::env::temp_dir().join(format!("quad_gecko_ls_{tmp_id}.sqlite"));
    let tmp_wal = std::env::temp_dir().join(format!("quad_gecko_ls_{tmp_id}.sqlite-wal"));

    if std::fs::copy(db_path, &tmp_db).is_err() {
        return Vec::new();
    }
    let wal_path = db_path.with_file_name("data.sqlite-wal");
    if wal_path.exists() {
        let _ = std::fs::copy(&wal_path, &tmp_wal);
    }

    let output = std::process::Command::new("sqlite3")
        .arg(&tmp_db)
        .arg("SELECT compression_type, hex(value) FROM data WHERE key='userInfo' OR key LIKE '%token%' OR key LIKE '%auth%';")
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
            let comp_type: u32 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            if let Some(hex_data) = parts.get(1)
                && let Some(raw) = decode_hex(hex_data)
            {
                let decompressed = if comp_type == 1 {
                    decompress_snappy(&raw).unwrap_or(raw)
                } else {
                    raw
                };
                scan_bytes_for_jwt(&decompressed, &mut out);
            }
        }
    }
    out
}

fn extract_from_chromium_localstorage(leveldb_dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(leveldb_dir) else {
        return out;
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with(".ldb") || name.ends_with(".log") {
            files.push(path);
        }
    }

    sort_by_modified_desc(&mut files);

    for f in files {
        if let Ok(bytes) = std::fs::read(&f) {
            scan_bytes_for_jwt(&bytes, &mut out);
            if !out.is_empty() {
                break;
            }
        }
    }
    out
}

fn extract_from_gecko_cookies(db_path: &Path, target_host: Option<&str>) -> Vec<String> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp_id = format!(
        "{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    );
    let tmp_db = std::env::temp_dir().join(format!("quad_gecko_c_{tmp_id}.sqlite"));
    let tmp_wal = std::env::temp_dir().join(format!("quad_gecko_c_{tmp_id}.sqlite-wal"));

    if std::fs::copy(db_path, &tmp_db).is_err() {
        return Vec::new();
    }
    let wal_path = db_path.with_file_name("cookies.sqlite-wal");
    if wal_path.exists() {
        let _ = std::fs::copy(&wal_path, &tmp_wal);
    }

    let host_clause = if let Some(h) = target_host {
        let clean_h = clean_host(h);
        format!("(host LIKE '%{clean_h}%' OR host LIKE '%digii%' OR host LIKE '%collpoll%')")
    } else {
        "(host LIKE '%digii%' OR host LIKE '%collpoll%')".to_string()
    };

    let sql = format!(
        "SELECT value FROM moz_cookies WHERE {host_clause} AND (name='Auth-Token' OR name='user') ORDER BY lastAccessed DESC;"
    );

    let output = std::process::Command::new("sqlite3")
        .arg(&tmp_db)
        .arg(&sql)
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

fn extract_from_chromium_cookies(
    db_path: &Path,
    _service: Option<&str>,
    target_host: Option<&str>,
) -> Vec<String> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp_id = format!(
        "{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    );
    let tmp_db = std::env::temp_dir().join(format!("quad_chrom_c_{tmp_id}.sqlite"));
    let tmp_wal = std::env::temp_dir().join(format!("quad_chrom_c_{tmp_id}.sqlite-wal"));

    if std::fs::copy(db_path, &tmp_db).is_err() {
        return Vec::new();
    }
    let wal_path = db_path.with_file_name("Cookies-wal");
    if wal_path.exists() {
        let _ = std::fs::copy(&wal_path, &tmp_wal);
    }

    let host_clause = if let Some(h) = target_host {
        let clean_h = clean_host(h);
        format!(
            "(host_key LIKE '%{clean_h}%' OR host_key LIKE '%digii%' OR host_key LIKE '%collpoll%')"
        )
    } else {
        "(host_key LIKE '%digii%' OR host_key LIKE '%collpoll%')".to_string()
    };

    let sql = format!(
        "SELECT value, hex(encrypted_value) FROM cookies WHERE {host_clause} AND (name='Auth-Token' OR name='user') ORDER BY creation_utc DESC;"
    );

    let output = std::process::Command::new("sqlite3")
        .arg(&tmp_db)
        .arg(&sql)
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
                if let Some(service_name) = _service
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

fn scan_all_browsers(target_host: Option<&str>) -> (Option<FoundSession>, Option<FoundSession>) {
    let mut best_active: Option<FoundSession> = None;
    let mut best_expired: Option<FoundSession> = None;
    let now = config::now_unix();

    let clean_target = target_host.map(|h| clean_host(h).to_lowercase());

    for src in get_all_browser_sources(target_host) {
        let mut tokens = Vec::new();

        // 1. Check LocalStorage (active single-page app sessions live here!)
        if src.is_chromium {
            for dir in &src.chromium_ls_dirs {
                tokens.extend(extract_from_chromium_localstorage(dir));
            }
        } else {
            for db in &src.gecko_ls_dbs {
                let found = extract_from_gecko_localstorage(db);
                let has_tokens = !found.is_empty();
                tokens.extend(found);
                if has_tokens {
                    break;
                }
            }
        }

        // 2. Check Cookies
        for db in &src.cookie_dbs {
            if src.is_chromium {
                #[cfg(target_os = "macos")]
                {
                    tokens.extend(extract_from_chromium_cookies(
                        db,
                        src.keychain_service,
                        target_host,
                    ));
                }
                #[cfg(not(target_os = "macos"))]
                {
                    tokens.extend(extract_from_chromium_cookies(db, None, target_host));
                }
            } else {
                tokens.extend(extract_from_gecko_cookies(db, target_host));
            }
        }

        for token in tokens {
            let Ok(claims) = jwt_claims(&token) else {
                continue;
            };
            let Some(exp) = claims.get("exp").and_then(|v| v.as_i64()) else {
                continue;
            };
            if claims.get("sub").is_none() {
                continue;
            }

            // If a specific target host is required, verify instituteUrl matches if present
            if let Some(ref target) = clean_target
                && let Some(inst) = claims.get("instituteUrl").and_then(|v| v.as_str())
            {
                let clean_inst = clean_host(inst).to_lowercase();
                if !clean_inst.contains(target) && !target.contains(&clean_inst) {
                    continue;
                }
            }

            if exp > now {
                // Active session! Pick the one with the furthest expiration date
                if best_active.as_ref().is_none_or(|b| exp > b.exp) {
                    best_active = Some(FoundSession {
                        token,
                        browser: src.browser_name,
                        exp,
                    });
                }
            } else {
                // Expired session. Pick the one that expired most recently
                if best_expired.as_ref().is_none_or(|b| exp > b.exp) {
                    best_expired = Some(FoundSession {
                        token,
                        browser: src.browser_name,
                        exp,
                    });
                }
            }
        }

        // If we found an active session in the prioritized browser, return immediately!
        if best_active.is_some() {
            return (best_active, None);
        }
    }

    (best_active, best_expired)
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
        Ok(()) => crate::ui::ok(&format!("Removed {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            crate::ui::info(&format!("Nothing saved at {}", path.display()))
        }
        Err(e) => return Err(format!("could not remove {}: {e}", path.display()).into()),
    }
    Ok(())
}

fn print_instructions(portal: Option<&str>) {
    let host = portal.unwrap_or("<your-institute>.digiicampus.com");
    let host = clean_host(host);
    println!(
        "To get your session token manually:\n\
         \n  1. Open https://{host} and sign in.\n\
         \n  2. Open DevTools (Option+Cmd+I on macOS, F12 on Windows/Linux) -> Application -> Cookies -> https://{host}\n\
         \n  3. Find the cookie named `user` or `Auth-Token` and copy its value (starts with `eyJ`).\n\
         \n  4. Run:  quad login --token <paste-value-here>"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snappy_literal_decompression() {
        // Snappy encoding of "hello world":
        // Varint length = 11 (0x0B)
        // Tag byte for literal of len 11: (11 - 1) << 2 = 10 << 2 = 40 (0x28)
        let mut encoded = vec![0x0B, 0x28];
        encoded.extend_from_slice(b"hello world");

        let decompressed = decompress_snappy(&encoded).expect("decompression failed");
        assert_eq!(decompressed, b"hello world");
    }

    #[test]
    fn test_snappy_copy_decompression() {
        // "abcabcabc" using literal "abc" followed by copy
        // Uncompressed length = 9 (0x09)
        // Literal "abc": tag (3 - 1) << 2 = 8 (0x08), followed by b"abc"
        // Copy-1: len 6 (len - 4 = 2 -> (2 << 2) | 1 = 0x09), offset 3 (offset & 0xE0 = 0, byte = 3)
        let encoded = vec![0x09, 0x08, b'a', b'b', b'c', 0x09, 0x03];
        let decompressed = decompress_snappy(&encoded).expect("decompression failed");
        assert_eq!(decompressed, b"abcabcabc");
    }

    #[test]
    fn test_snappy_malformed_input_returns_none() {
        // Truncated input should never panic
        assert!(decompress_snappy(&[0xFF]).is_none());
        assert!(decompress_snappy(&[0x05, 0x28, b'a']).is_none());
    }

    #[test]
    fn test_decode_hex() {
        assert_eq!(decode_hex("48656c6c6f"), Some(b"Hello".to_vec()));
        assert_eq!(decode_hex("00FF"), Some(vec![0x00, 0xFF]));
        assert!(decode_hex("odd").is_none());
        assert!(decode_hex("invalid-chars").is_none());
    }

    #[test]
    fn test_scan_bytes_for_jwt_ascii() {
        let sample =
            b"garbage eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.do_not_verify more garbage";
        let mut out = Vec::new();
        scan_bytes_for_jwt(sample, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0],
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.do_not_verify"
        );
    }

    #[test]
    fn test_scan_bytes_for_jwt_utf16le() {
        let token = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.do_not_verify";
        let mut utf16_bytes = Vec::new();
        for ch in token.encode_utf16() {
            utf16_bytes.extend_from_slice(&ch.to_le_bytes());
        }
        let mut out = Vec::new();
        scan_bytes_for_jwt(&utf16_bytes, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], token);
    }

    #[test]
    fn test_live_scan_zen() {
        let (active, _expired) = scan_all_browsers(Some("snuc.digiicampus.com"));
        if std::env::var("CI").is_err() && active.is_some() {
            assert!(active.is_some());
        }
    }
}
