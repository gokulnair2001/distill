//! JavaScript rendering via a headless browser.
//!
//! We shell out to an installed Chrome/Chromium with `--headless --dump-dom`,
//! which prints the fully-rendered DOM to stdout after JS executes. This keeps
//! the binary lean (no CDP/browser-automation crate) and lets us reuse the exact
//! same static pipeline on the rendered HTML.
//!
//! `find_browser()` honours `$DISTILL_CHROME` first, then common install paths.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Locate a usable Chromium-family browser binary.
pub fn find_browser() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("DISTILL_CHROME") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    let candidates = [
        // macOS app bundles
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        // Linux
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/usr/bin/brave-browser",
        "/usr/bin/microsoft-edge",
    ];
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

/// Render `url` with a headless browser and return the post-JS DOM as HTML.
///
/// `budget_ms` is the virtual-time budget given to the page for JS/network.
/// Returns `None` if no browser is found or rendering fails/times out.
pub fn render(url: &str, budget_ms: u32) -> Option<String> {
    let browser = find_browser()?;
    let debug = std::env::var("DISTILL_DEBUG").is_ok();
    if debug {
        eprintln!("[debug] rendering via {}", browser.display());
    }

    let mut child = Command::new(&browser)
        .args([
            "--headless=new",
            "--disable-gpu",
            "--no-sandbox",
            "--disable-dev-shm-usage",
            "--hide-scrollbars",
            "--no-first-run",
            "--disable-extensions",
            &format!("--virtual-time-budget={budget_ms}"),
            "--dump-dom",
            url,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    // Read stdout on a thread so a hung browser can't block us forever.
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        let _ = tx.send(buf);
    });

    // Hard wall-clock cap = virtual-time budget + generous headroom.
    let hard_cap = Duration::from_millis(budget_ms as u64 + 12_000);
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() > hard_cap {
                    let _ = child.kill();
                    if debug {
                        eprintln!("[debug] render timed out; killed browser");
                    }
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }

    let dom = rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    if dom.trim().is_empty() {
        None
    } else {
        Some(dom)
    }
}
