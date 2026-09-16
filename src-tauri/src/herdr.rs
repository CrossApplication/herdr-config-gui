//! Locating and invoking the herdr binary.
//!
//! A GUI app launched from Finder/Explorer does not inherit the user's shell
//! PATH, so `Command::new("herdr")` alone is not enough. We probe the usual
//! install locations documented by herdr (installer script, Homebrew, mise).

use std::path::PathBuf;
use std::process::Command;

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = home_dir() {
        out.push(home.join(".local/bin/herdr"));
        out.push(home.join(".local/share/mise/shims/herdr"));
        out.push(home.join(".cargo/bin/herdr"));
    }
    for p in ["/opt/homebrew/bin/herdr", "/usr/local/bin/herdr", "/usr/bin/herdr"] {
        out.push(PathBuf::from(p));
    }
    out
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Resolve the herdr executable: PATH first, then known install locations.
pub fn resolve() -> Option<PathBuf> {
    if Command::new("herdr").arg("--version").output().is_ok() {
        return Some(PathBuf::from("herdr"));
    }
    candidates().into_iter().find(|p| p.is_file())
}

pub fn run(args: &[&str]) -> Result<String, String> {
    let exe = resolve().ok_or_else(|| "herdr executable not found".to_string())?;
    let out = Command::new(&exe)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run {}: {e}", exe.display()))?;
    if !out.status.success() {
        return Err(format!(
            "{} {:?} exited with {}: {}",
            exe.display(),
            args,
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn version() -> Option<String> {
    run(&["--version"]).ok().map(|s| s.trim().to_string())
}

/// `herdr --default-config` is the schema source of truth.
pub fn default_config() -> Result<String, String> {
    run(&["--default-config"])
}
