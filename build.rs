//! Sets `ZONEQ_VERSION` to the short git SHA, or `unknown` outside a checkout.

use std::path::Path;
use std::process::Command;

fn main() {
    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=ZONEQ_VERSION={sha}");

    // Rebuild when HEAD moves. A missing path makes cargo rerun this every
    // build, so only watch what exists.
    for path in [".git/HEAD", ".git/refs/heads", ".git/packed-refs"] {
        if Path::new(path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}
