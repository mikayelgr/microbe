//! Microbe — a lightweight, extensible MicroPython IDE.
//!
//! Extension points (see each module):
//! * `platforms/*.toml` — describe a MicroPython port declaratively.
//! * [`transport::TransportProvider`] — a new way to reach a REPL.
//! * [`firmware::flashers::Flasher`] / [`firmware::detect::Detector`] — flashing.
//! * [`lsp::LanguageServerProvider`] — a different Python language server.
//! * `py/*.py` — device-side scripts, overridable per platform.
//! * [`analysis::LintRule`] / [`analysis::DiagnosticFilter`] — static checks.
//! * [`mcp::tools::McpTool`] — a tool AI agents can call over MCP.

pub mod analysis;
pub mod cache;
pub mod commands;
pub mod debug;
pub mod device_info;
pub mod error;
pub mod firmware;
pub mod fs;
pub mod lsp;
pub mod mcp;
pub mod mip;
pub mod platform;
pub mod progress;
pub mod repl;
pub mod scripts;
pub mod state;
pub mod sync;
pub mod testing;
pub mod tools;
pub mod transport;

use tauri::Manager;

use commands::{device, files, project};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            let paths = state::Paths {
                data: app.path().app_data_dir()?,
                cache: app.path().app_cache_dir()?,
            };
            app.manage(state::AppState::new(paths)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            device::transports_list,
            device::platforms_list,
            device::device_discover,
            device::device_connect,
            device::device_identify,
            device::device_sessions,
            device::device_disconnect,
            device::device_write,
            device::device_interrupt,
            device::device_soft_reset,
            device::device_hard_reset,
            device::device_run,
            device::device_stop,
            device::device_exec,
            device::device_script,
            device::ble_helper_install,
            device::device_rescue,
            files::fs_list,
            files::fs_read_text,
            files::fs_write_text,
            files::fs_upload,
            files::fs_download,
            files::fs_mkdir,
            files::fs_remove,
            files::fs_rename,
            files::project_sync,
            project::project_list,
            project::project_read,
            project::project_write,
            project::project_create,
            project::project_remove,
            project::project_rename,
            firmware::firmware_index,
            firmware::firmware_board,
            firmware::firmware_detect,
            firmware::firmware_flash,
            firmware::esp32::esp32_partitions,
            firmware::builder::firmware_builders,
            firmware::builder::firmware_build,
            firmware::builder::firmware_build_cancel,
            firmware::esp32::esp32_image_partitions,
            mip::packages_index,
            mip::packages_install,
            lsp::lsp_providers,
            lsp::stubs_ensure,
            lsp::lsp_start,
            lsp::lsp_send,
            lsp::lsp_set_stubs,
            lsp::lsp_stop,
            analysis::analysis_checks,
            analysis::analysis_run,
            testing::tests_discover,
            debug::debug_instrument,
            mcp::mcp_status,
            mcp::mcp_start,
            mcp::mcp_stop,
            mcp::mcp_regenerate_token,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Microbe");
}
