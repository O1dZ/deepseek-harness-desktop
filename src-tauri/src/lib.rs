mod local_log;
mod runtime;
mod settings;

use crate::{
    local_log::LocalLog,
    runtime::{RuntimeSupervisor, ShellSnapshot},
    settings::{configure_autostart, AppPaths, DesktopSettings},
};
use std::{process::Command, sync::Arc, thread, time::Duration};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, RunEvent, State, WindowEvent,
};

#[tauri::command]
fn get_shell_state(supervisor: State<'_, Arc<RuntimeSupervisor>>) -> ShellSnapshot {
    supervisor.snapshot()
}

#[tauri::command]
fn get_desktop_settings(
    supervisor: State<'_, Arc<RuntimeSupervisor>>,
) -> Result<DesktopSettings, String> {
    DesktopSettings::load(supervisor.paths()).map_err(format_error)
}

#[tauri::command]
fn choose_workspace(
    supervisor: State<'_, Arc<RuntimeSupervisor>>,
) -> Result<Option<String>, String> {
    let folder = rfd::FileDialog::new()
        .set_title("选择 DeepSeek Harness Workspace")
        .pick_folder();
    let Some(folder) = folder else {
        return Ok(None);
    };
    let mut settings = DesktopSettings::load(supervisor.paths()).map_err(format_error)?;
    settings.workspace = Some(folder.clone());
    settings.save(supervisor.paths()).map_err(format_error)?;
    supervisor.restart().map_err(format_error)?;
    Ok(Some(folder.display().to_string()))
}

#[tauri::command]
fn restart_runtime(supervisor: State<'_, Arc<RuntimeSupervisor>>) -> Result<(), String> {
    supervisor.restart().map_err(format_error)
}

#[tauri::command]
fn save_desktop_settings(
    supervisor: State<'_, Arc<RuntimeSupervisor>>,
    settings: DesktopSettings,
) -> Result<(), String> {
    configure_autostart(settings.launch_at_login).map_err(format_error)?;
    settings.save(supervisor.paths()).map_err(format_error)?;
    Ok(())
}

#[tauri::command]
fn open_logs(supervisor: State<'_, Arc<RuntimeSupervisor>>) -> Result<(), String> {
    Command::new("explorer.exe")
        .arg(supervisor.log().directory())
        .spawn()
        .map(|_| ())
        .map_err(format_error)
}

#[tauri::command]
fn clear_logs(supervisor: State<'_, Arc<RuntimeSupervisor>>) -> Result<(), String> {
    supervisor.log().clear().map_err(format_error)
}

#[tauri::command]
fn open_desktop_settings(app: AppHandle) -> Result<(), String> {
    show_settings_window(&app).map_err(format_error)
}

fn show_settings_window(app: &AppHandle) -> anyhow::Result<()> {
    let window = app
        .get_webview_window("desktop-settings")
        .ok_or_else(|| anyhow::anyhow!("desktop settings window is unavailable"))?;
    window.show()?;
    window.set_focus()?;
    Ok(())
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn build_tray(app: &AppHandle, supervisor: Arc<RuntimeSupervisor>) -> anyhow::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开 DeepSeek Harness", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "桌面设置", true, None::<&str>)?;
    let logs = MenuItem::with_id(app, "logs", "打开日志", true, None::<&str>)?;
    let restart = MenuItem::with_id(app, "restart", "重启 Runtime", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "彻底退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &settings, &logs, &restart, &quit])?;

    let app_handle = app.clone();
    TrayIconBuilder::with_id("main-tray")
        .tooltip(format!(
            "DeepSeek Harness Desktop {}",
            runtime::edition_name()
        ))
        .icon(
            app.default_window_icon()
                .cloned()
                .expect("application icon"),
        )
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "settings" => {
                let _ = show_settings_window(app);
            }
            "logs" => {
                let _ = Command::new("explorer.exe")
                    .arg(supervisor.log().directory())
                    .spawn();
            }
            "restart" => {
                let supervisor = supervisor.clone();
                thread::spawn(move || {
                    let _ = supervisor.restart();
                });
            }
            "quit" => {
                supervisor.shutdown(Duration::from_secs(5));
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(move |_tray, event| {
            if matches!(event, tauri::tray::TrayIconEvent::DoubleClick { .. }) {
                show_main(&app_handle);
            }
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app)
        }))
        .invoke_handler(tauri::generate_handler![
            get_shell_state,
            get_desktop_settings,
            choose_workspace,
            restart_runtime,
            save_desktop_settings,
            open_logs,
            clear_logs,
            open_desktop_settings,
        ])
        .setup(|app| {
            let resolver = app.path();
            let paths = AppPaths {
                config_dir: resolver.app_config_dir()?,
                data_dir: resolver.app_data_dir()?,
                log_dir: resolver.app_log_dir()?,
                resource_dir: resolver.resource_dir()?,
            };
            let log = LocalLog::open(paths.log_dir.clone())?;
            log.write(
                "desktop",
                &format!(
                    "Starting Desktop {} {} with dsh {}",
                    runtime::edition_name(),
                    env!("CARGO_PKG_VERSION"),
                    runtime::DSH_VERSION,
                ),
            );
            let supervisor = RuntimeSupervisor::new(paths, log);
            supervisor.attach_app(app.handle().clone());
            app.manage(supervisor.clone());
            build_tray(app.handle(), supervisor.clone())?;

            if let Some(window) = app.get_webview_window("main") {
                let window_for_event = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window_for_event.hide();
                    }
                });
            }

            if let Some(window) = app.get_webview_window("desktop-settings") {
                let window_for_event = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window_for_event.hide();
                    }
                });
            }

            let hidden = std::env::args().any(|arg| arg == "--hidden");
            if hidden {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            thread::spawn(move || {
                thread::sleep(Duration::from_millis(250));
                let _ = supervisor.start_saved(true);
            });
            Ok(())
        });

    let app = builder
        .build(tauri::generate_context!())
        .expect("failed to build DeepSeek Harness Desktop");
    app.run(|app, event| {
        if let RunEvent::ExitRequested { .. } = event {
            if let Some(supervisor) = app.try_state::<Arc<RuntimeSupervisor>>() {
                supervisor.shutdown(Duration::from_secs(5));
            }
        }
    });
}

fn format_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
