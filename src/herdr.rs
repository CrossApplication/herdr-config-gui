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
    /// `PATH`, searched by hand on Windows. See `resolve`.
    pub path: Option<std::ffi::OsString>,
}

impl BinEnv {
    pub fn from_process() -> Self {
        Self {
            windows: cfg!(windows),
            home: home_dir(),
            local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            path: std::env::var_os("PATH"),
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

/// `herdr.exe` in each PATH directory, in order. Windows only.
///
/// Relative entries such as `.` are skipped: they would resolve against
/// whatever the current directory happens to be, which is the kind of lookup
/// `resolve` exists to avoid.
fn path_candidates(env: &BinEnv) -> Vec<PathBuf> {
    let Some(path) = &env.path else {
        return Vec::new();
    };
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("herdr.exe"))
        .collect()
}

/// The first herdr that exists on Windows: PATH, then the install locations.
fn find_on_windows(env: &BinEnv) -> Option<PathBuf> {
    path_candidates(env)
        .into_iter()
        .chain(candidate_paths(env))
        .chain(windows_release_dirs(env))
        .find(|p| p.is_file())
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

/// A `Command` for herdr that does not flash a console window on Windows.
///
/// The app is a GUI-subsystem program there, so it has no console of its own
/// to share. Windows opens a fresh console for every console program such a
/// process starts unless told not to, and herdr is one. Output is captured
/// through pipes either way.
fn command(exe: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Resolve the herdr executable: PATH first, then documented install locations.
///
/// Windows is searched by hand and always yields a full path. Given a bare
/// name, Rust's standard library looks in the directory holding this app's
/// own exe before PATH, so a `herdr.exe` sitting next to it -- in Downloads,
/// say, where the exe is likely to be run from -- would be run in place of
/// the real one, at startup and on every save.
pub fn resolve() -> Option<PathBuf> {
    let env = BinEnv::from_process();
    if env.windows {
        return find_on_windows(&env);
    }
    if command("herdr").arg("--version").output().is_ok() {
        return Some(PathBuf::from("herdr"));
    }
    candidate_paths(&env).into_iter().find(|p| p.is_file())
}

pub fn run(args: &[&str]) -> Result<String, String> {
    let exe = resolve().ok_or_else(|| "herdr executable not found".to_string())?;
    let out = command(&exe)
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
    let out = command(&exe)
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
            path: None,
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
            path: None,
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
            path: None,
        });
        assert!(!unix.is_empty(), "system paths are still probed");
        let win = candidate_paths(&BinEnv {
            windows: true,
            home: None,
            local_app_data: None,
            path: None,
        });
        assert!(
            win.is_empty(),
            "nothing is guessable on Windows without a profile"
        );
    }

    #[test]
    fn windows_runs_herdr_from_path_not_from_beside_the_app() {
        let root = std::env::temp_dir().join(format!("herdr-gui-lookup-{}", std::process::id()));
        let app_dir = root.join("Downloads");
        let on_path = root.join("bin");
        let elsewhere = root.join("other");
        for dir in [&app_dir, &on_path, &elsewhere] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let env = |dirs: &[&PathBuf]| BinEnv {
            windows: true,
            home: None,
            local_app_data: None,
            path: Some(std::env::join_paths(dirs).unwrap()),
        };

        // A herdr.exe beside the app, and nowhere else: not found.
        std::fs::write(app_dir.join("herdr.exe"), "").unwrap();
        let beside_only = find_on_windows(&env(&[&elsewhere]));
        // The real one on PATH is preferred to the one beside the app.
        std::fs::write(on_path.join("herdr.exe"), "").unwrap();
        let both = find_on_windows(&env(&[&elsewhere, &on_path]));
        // A relative PATH entry is never searched.
        let relative = path_candidates(&BinEnv {
            path: Some(std::env::join_paths([".", "bin"]).unwrap()),
            ..env(&[])
        });
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(beside_only, None);
        assert_eq!(both, Some(on_path.join("herdr.exe")));
        assert!(relative.is_empty(), "{relative:?}");
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
