#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod herdr;
mod schema;

use serde::Serialize;

#[derive(Serialize)]
struct Bootstrap {
    herdr_found: bool,
    herdr_path: Option<String>,
    herdr_version: Option<String>,
    schema: Option<schema::Schema>,
    schema_error: Option<String>,
    config: config::ConfigState,
    /// Paths present in config.toml that the derived schema does not know
    /// about -- newer herdr versions, plugins, or open-ended tables.
    unknown_paths: Vec<String>,
}

#[tauri::command]
fn bootstrap() -> Bootstrap {
    let exe = herdr::resolve();
    let cfg = config::load();

    let (schema, schema_error) = match herdr::default_config() {
        Ok(text) => (Some(schema::parse(&text)), None),
        Err(e) => (None, Some(e)),
    };

    let known: Vec<String> = schema
        .as_ref()
        .map(|s| s.sections.iter().flat_map(|sec| sec.items.iter().map(|i| i.path.clone())).collect())
        .unwrap_or_default();

    // An open-ended table (e.g. [theme.custom]) accepts arbitrary keys, so a
    // path is also "known" when its parent table is part of the schema.
    let known_sections: Vec<String> = schema
        .as_ref()
        .map(|s| s.sections.iter().map(|sec| sec.name.clone()).collect())
        .unwrap_or_default();

    let unknown_paths = cfg
        .set_paths
        .iter()
        .filter(|p| {
            if known.contains(p) {
                return false;
            }
            match p.rsplit_once('.') {
                Some((parent, _)) => !known_sections.iter().any(|s| s == parent),
                None => true,
            }
        })
        .cloned()
        .collect();

    Bootstrap {
        herdr_found: exe.is_some(),
        herdr_path: exe.map(|p| p.display().to_string()),
        herdr_version: herdr::version(),
        schema,
        schema_error,
        config: cfg,
        unknown_paths,
    }
}

#[tauri::command]
fn preview_edits(edits: Vec<config::Edit>) -> config::Preview {
    config::preview(edits)
}

#[tauri::command]
fn save_edits(edits: Vec<config::Edit>, reload: bool) -> Result<config::SaveResult, String> {
    config::save(edits, reload)
}

fn main() {
    // Headless check of the exact payload the UI receives. Useful in CI and
    // when the window cannot be inspected directly.
    if std::env::var_os("HERDR_GUI_DUMP").is_some() {
        println!("{}", serde_json::to_string_pretty(&bootstrap()).unwrap());
        return;
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![bootstrap, preview_edits, save_edits])
        .run(tauri::generate_context!())
        .expect("error while running herdr-config-gui");
}
