//! Locating and invoking the herdr binary.
//!
//! A GUI app launched from Finder/Explorer does not inherit the user's shell
//! PATH, so `Command::new("herdr")` alone is not enough. We probe the install
//! locations herdr documents for each platform.

use std::path::PathBuf;
use std::process::Command;

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// The environment that decides where herdr might be installed. Separated from
/// the process so the per-platform lists can be tested on any host.
#[derive(Clone, Default, Debug)]
pub struct BinEnv {
    pub windows: bool,
    pub home: Option<PathBuf>,
    /// `%LOCALAPPDATA%`, Windows only.
    pub local_app_data: Option<PathBuf>,
}

impl BinEnv {
    pub fn from_process() -> Self {
        Self {
            windows: cfg!(windows),
            home: home_dir(),
            local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        }
    }
}

/// Fixed paths worth probing, in priority order.
///
/// Windows locations come from herdr's own install docs: the versioned
/// standalone release directory under the profile, plus a stable compatibility
/// alias under LOCALAPPDATA. The executable needs its `.exe` suffix here,
/// because only PATH lookups get PATHEXT applied for free.
pub fn candidate_paths(env: &BinEnv) -> Vec<PathBuf> {
    let exe = if env.windows { "herdr.exe" } else { "herdr" };
    let mut out = Vec::new();

    if env.windows {
        if let Some(lad) = &env.local_app_data {
            out.push(lad.join("Programs").join("Herdr").join("bin").join(exe));
        }
        if let Some(home) = &env.home {
            out.push(home.join(".local").join("bin").join(exe));
        }
    } else {
        if let Some(home) = &env.home {
            out.push(home.join(".local/bin").join(exe));
            out.push(home.join(".local/share/mise/shims").join(exe));
            out.push(home.join(".cargo/bin").join(exe));
        }
        for p in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
            out.push(PathBuf::from(p).join(exe));
        }
    }
    out
}

/// The Windows installer keeps every version it has downloaded, so the newest
/// release directory has to be found rather than named.
fn windows_release_dirs(env: &BinEnv) -> Vec<PathBuf> {
    let Some(home) = &env.home else {
        return Vec::new();
    };
    let releases = home
        .join(".herdr")
        .join("packages")
        .join("standalone")
        .join("releases");
    let Ok(entries) = std::fs::read_dir(&releases) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    // Newest first. Version names sort usefully enough for a fallback probe.
    dirs.sort();
    dirs.reverse();
    dirs.into_iter().map(|d| d.join("herdr.exe")).collect()
}

/// Resolve the herdr executable: PATH first, then documented install locations.
pub fn resolve() -> Option<PathBuf> {
    if Command::new("herdr").arg("--version").output().is_ok() {
        return Some(PathBuf::from("herdr"));
    }
    let env = BinEnv::from_process();
    candidate_paths(&env)
        .into_iter()
        .chain(if env.windows {
            windows_release_dirs(&env)
        } else {
            Vec::new()
        })
        .find(|p| p.is_file())
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

/// Run herdr against a specific config file and capture its report whether or
/// not it succeeds. `herdr config check` exits non-zero when it finds issues,
/// and that output is exactly what we want to read.
pub fn run_against_config(args: &[&str], config: &std::path::Path) -> Result<String, String> {
    let exe = resolve().ok_or_else(|| "herdr executable not found".to_string())?;
    let out = Command::new(&exe)
        .args(args)
        .env("HERDR_CONFIG_PATH", config)
        .output()
        .map_err(|e| format!("failed to run {}: {e}", exe.display()))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        text.push_str(&err);
    }
    Ok(text)
}

pub fn version() -> Option<String> {
    run(&["--version"]).ok().map(|s| s.trim().to_string())
}

/// `herdr --default-config` is the schema source of truth.
pub fn default_config() -> Result<String, String> {
    run(&["--default-config"])
}

#[derive(Default, Debug, PartialEq)]
pub struct ResolvedPaths {
    pub config: Option<String>,
    pub log: Option<String>,
}

/// `herdr --help` ends with the paths it resolved for this machine:
///
/// ```text
/// Config: /Users/me/.config/herdr/config.toml
/// Logs:   /Users/me/.config/herdr/herdr.log (plus ...)
/// Env:    HERDR_CONFIG_PATH overrides config file path
/// ```
///
/// Asking herdr beats reimplementing its rules: it already accounts for
/// `HERDR_CONFIG_PATH`, `XDG_CONFIG_HOME` and the Windows `%APPDATA%` layout.
pub fn parse_help_paths(help: &str) -> ResolvedPaths {
    let mut out = ResolvedPaths::default();
    for line in help.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Config:") {
            out.config = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("Logs:") {
            // Drop the trailing "(plus herdr-client.log, ...)" note.
            let path = rest.trim().split(" (plus").next().unwrap_or("").trim();
            if !path.is_empty() {
                out.log = Some(path.to_string());
            }
        }
    }
    out
}

/// The config file herdr itself would read, or None when herdr is unavailable.
pub fn resolved_config_path() -> Option<PathBuf> {
    let help = run(&["--help"]).ok()?;
    parse_help_paths(&help).config.map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_candidates_carry_the_exe_suffix() {
        let env = BinEnv {
            windows: true,
            home: Some(PathBuf::from("C:/Users/me")),
            local_app_data: Some(PathBuf::from("C:/Users/me/AppData/Local")),
        };
        let paths = candidate_paths(&env);
        assert!(
            paths.iter().all(|p| p.file_name().unwrap() == "herdr.exe"),
            "every Windows candidate needs .exe: {paths:?}"
        );
        // The documented compatibility alias is the stable one, so probe it first.
        assert_eq!(
            paths[0],
            PathBuf::from("C:/Users/me/AppData/Local/Programs/Herdr/bin/herdr.exe")
        );
        assert!(paths.iter().any(|p| p.starts_with("C:/Users/me/.local")));
        // No Unix locations leak in.
        assert!(!paths.iter().any(|p| p.starts_with("/opt")));
    }

    #[test]
    fn unix_candidates_have_no_exe_suffix() {
        let env = BinEnv {
            windows: false,
            home: Some(PathBuf::from("/home/me")),
            local_app_data: None,
        };
        let paths = candidate_paths(&env);
        assert!(paths.iter().all(|p| p.file_name().unwrap() == "herdr"));
        assert!(paths.contains(&PathBuf::from("/home/me/.local/bin/herdr")));
        assert!(paths.contains(&PathBuf::from("/opt/homebrew/bin/herdr")));
        assert!(paths.contains(&PathBuf::from("/usr/local/bin/herdr")));
    }

    #[test]
    fn candidates_survive_a_missing_home() {
        let unix = candidate_paths(&BinEnv {
            windows: false,
            home: None,
            local_app_data: None,
        });
        assert!(!unix.is_empty(), "system paths are still probed");
        let win = candidate_paths(&BinEnv {
            windows: true,
            home: None,
            local_app_data: None,
        });
        assert!(
            win.is_empty(),
            "nothing is guessable on Windows without a profile"
        );
    }

    #[test]
    fn help_output_yields_the_resolved_paths() {
        let help = "\
Options:
  --help, -h          Show this help

Config: /Users/me/.config/herdr/config.toml
Logs:   /Users/me/.config/herdr/herdr.log (plus herdr-client.log, herdr-server.log)
Env:    HERDR_CONFIG_PATH overrides config file path
";
        let p = parse_help_paths(help);
        assert_eq!(
            p.config.as_deref(),
            Some("/Users/me/.config/herdr/config.toml")
        );
        assert_eq!(p.log.as_deref(), Some("/Users/me/.config/herdr/herdr.log"));
    }

    #[test]
    fn help_output_yields_windows_paths_too() {
        let help = "Config: C:\\Users\\me\\AppData\\Roaming\\herdr\\config.toml\n";
        assert_eq!(
            parse_help_paths(help).config.as_deref(),
            Some("C:\\Users\\me\\AppData\\Roaming\\herdr\\config.toml")
        );
    }

    #[test]
    fn help_without_the_paths_block_yields_nothing() {
        assert_eq!(
            parse_help_paths("Usage: herdr [options]\n"),
            ResolvedPaths::default()
        );
    }
}
