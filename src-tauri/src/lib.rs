//! Beatshelf desktop shell: wires beatcore to the UI.

mod commands;
mod state;
mod watcher;

use beatcore::db::Db;
use state::AppState;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db_path = dir.join("library.sqlite");
            let db = Db::open(&db_path).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
            app.manage(AppState {
                db_path,
                db: Mutex::new(db),
                plans: Mutex::new(HashMap::new()),
                watcher: Mutex::new(None),
                scanning: AtomicBool::new(false),
                drops: Mutex::new(HashMap::new()),
                dragging_out: AtomicBool::new(false),
            });
            let handle = app.handle().clone();
            let state = handle.state::<AppState>();
            let cfg = state.config();
            if cfg.onboarded() {
                commands::allow_scope(&handle, &cfg);
                commands::restart_watcher(&handle, &state, &cfg);
                commands::spawn_scan(&handle, &state);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::DragDrop(e) = event {
                commands::on_drag_drop(window, e);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_state,
            commands::list_templates,
            commands::preview_template,
            commands::plan_create_library,
            commands::execute_plan,
            commands::discard_plan,
            commands::list_beats,
            commands::update_beat,
            commands::beat_detail,
            commands::plan_align_folder,
            commands::list_inbox,
            commands::set_inbox_ignored,
            commands::plan_sort,
            commands::list_history,
            commands::plan_undo,
            commands::asset_media,
            commands::inbox_media,
            commands::reveal,
            commands::set_prefs,
            commands::set_watch_folders,
            commands::rescan,
            commands::pick_files,
            commands::plan_add_files,
            commands::start_drag,
            commands::update_template,
            commands::render_preview,
            commands::row_peaks,
            commands::library_counts,
            commands::plan_set_cover,
            commands::plan_remove_cover,
            commands::export_mp3,
            commands::pick_import,
            commands::create_item,
            commands::delete_draft,
            commands::plan_delete_item,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Beatshelf");
}
