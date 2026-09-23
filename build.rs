//! Sets `ZONEQ_VERSION` to the version CI publishes: `0.2.<commit count>+<sha>`.
//!
//! Published crates have it stamped into `Cargo.toml`, since a crates.io
//! install builds without a `.git` to ask. Anywhere else it comes from git,
//! and outside a checkout it's the unstamped `CARGO_PKG_VERSION`.

use std::path::Path;
use std::process::Command;

fn main() {
    let pkg = env!("CARGO_PKG_VERSION");
    let version = if pkg.contains('+') {
        pkg.to_string()
    } else {
        git(&["rev-list", "--count", "HEAD"])
            .zip(git(&["rev-parse", "--short", "HEAD"]))
            .map(|(count, sha)| format!("0.2.{count}+{sha}"))
            .unwrap_or_else(|| pkg.to_string())
    };
    println!("cargo:rustc-env=ZONEQ_VERSION={version}");

    // Rebuild when HEAD moves. A missing path makes cargo rerun this every
    // build, so only watch what exists.
    for path in [".git/HEAD", ".git/refs/heads", ".git/packed-refs"] {
        if Path::new(path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

fn git(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
