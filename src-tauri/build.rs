#[path = "google_client_check.rs"]
mod google_client_check;

use google_client_check::{google_client_check, Check, WATCHED};

fn main() {
    // A store build never ships without its platform's Google OAuth client (2026-09-30).
    for name in WATCHED {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let profile = std::env::var("PROFILE").unwrap_or_default();
    match google_client_check(&target_os, &profile, &|name| std::env::var(name).ok()) {
        Check::Fine => {}
        Check::Warn(message) => println!("cargo:warning={message}"),
        Check::Fail(message) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    }
    tauri_build::build()
}
