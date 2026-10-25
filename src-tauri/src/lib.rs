mod app;
mod block;
mod commands;
mod error;
mod format;
mod mc;
mod pack;
mod render;
mod schematic;

// 单元测试共用的小工具：造 zip / PNG、按用例隔离的临时目录。
#[cfg(test)]
#[path = "tests/support.rs"]
pub(crate) mod test_support;

mod logger;


// 针对任意真实投影集合的端到端核对，按需运行（标了 #[ignore]）
#[cfg(test)]
#[path = "tests/real_project.rs"]
mod real_project;

/// 运行日志所在目录：应用数据目录下的 `logs`。
///
/// 与界面状态（配置目录）分开：日志是运行期产物，会随滚动不断累积，
/// 不该和用户设置混在一个目录里。
fn log_dir(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    app.path().app_data_dir().ok().map(|d| d.join("logs"))
}

/// 应用入口。以库形式导出，便于将来接入移动端入口。
///
/// 启动失败必须有可见反馈：发布版以 `windows_subsystem = "windows"`
/// 编译、没有控制台，错误信息无处可见，用户双击后只会看到
/// 「什么都没发生」。实测 WebView2 用户数据目录损坏时正是这种表现。
///
/// 因此同时装两道防线：
///   - panic 钩子：Tauri 建窗口失败时是 panic，不是返回 Err
///   - `run()` 返回 Err 时的处理：覆盖不 panic 的失败路径
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 先取配置：标识符要交给 panic 钩子与失败报告，好让提示里的数据目录与
    // 实际使用的一致，不必在别处再写死一份。代价是钩子不再最先安装，
    // generate_context!() 自身若 panic 就无人兜底；配置错误在 cargo build 时
    // 就被 tauri-codegen 拦下，这里判断风险可接受。
    let context = tauri::generate_context!();
    let identifier = context.config().identifier.clone();
    app::startup::install_panic_hook(identifier.clone());

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            // 日志要尽早打开：数据目录搬迁与后续启动流程都可能失败，
            // 而失败的现场只有日志能留下。打不开也不阻断启动——日志不可用
            // 是遗憾，程序起不来才是故障。
            //
            // 但也不能只有标准错误：发布版是 windows_subsystem = "windows"，
            // 没有控制台，用户什么都看不到，于是「日志功能整个没起来」这件事
            // 会一直没人发现。这里复用启动失败那条消息框通道（同样受
            // SCHEMATIC_MANAGER_NO_ERROR_DIALOG 抑制），让用户能看见并处理。
            match log_dir(app.handle()) {
                Some(dir) => match app::run_log::open(&dir) {
                    Ok(()) => {
                        logger::init_log_object(&dir);
                        logger_info!("SYSTEM", "The system is activated at system time {} .", &std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos().to_string());

                        // 等级先按界面上次存下的取值，debug 用户才能从第一行起
                        // 就有完整日志，而不是等界面把等级推过来才开始记
                        app::run_log::set_level(commands::log::saved_log_level(app.handle()));
                        let info = app.package_info();
                        app::run_log::info(&format!(
                            "启动 {} {}，数据目录 {}",
                            info.name,
                            info.version,
                            dir.parent()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default()
                        ));
                    }
                    Err(e) => {
                        let text = format!(
                            "运行日志无法启用，本次运行的日志不会写入。\n\n目录：{}\n原因：{e}\n\n请检查该目录是否可写。",
                            dir.display()
                        );
                        eprintln!("{text}");
                        app::startup::show_message(&text);
                    }
                },
                None => {
                    let text = "运行日志无法启用，本次运行的日志不会写入：定位应用数据目录失败。";
                    eprintln!("{text}");
                    app::startup::show_message(text);
                }
            }

            // 应用数据被读写前最早的时机：窗口已由 Tauri 建好（webview 的数据
            // 目录那时也建了），但前端命令要等事件循环才开始，界面状态与方块库
            // 都还没被碰过
            if let Err(e) = app::data_dir::migrate_legacy_data(app.handle()) {
                // 搬迁失败会让用户以为「更新完东西全没了」，先记下来再向上报
                app::run_log::error(&format!("应用数据目录搬迁失败：{e}"));
                return Err(e.into());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::schematic::peek_schematic,
            commands::schematic::scan_litematics,
            commands::schematic::analyze_schematic,
            commands::blocks::render_block_icons,
            commands::schematic::mc_version_name,
            commands::schematic::build_scene,
            commands::files::list_backups,
            commands::files::restore_backup,
            commands::files::delete_backup,
            commands::log::configure_run_log,
            commands::log::read_run_log,
            commands::log::clear_run_log,
            commands::log::open_run_log_dir,
            commands::log::log_ui_event,
            commands::log::run_log_dir,
            commands::blocks::search_blocks,
            commands::blocks::list_blocks,
            commands::blocks::list_block_versions,
            commands::blocks::preview_replace,
            commands::blocks::apply_replace,
            commands::duplicates::scan_duplicates,
            commands::convert::convert_targets,
            commands::convert::analyze_conversion,
            commands::convert::apply_conversion,
            commands::schematic::save_metadata,
            commands::schematic::save_schematic_as,
            commands::files::delete_files,
            commands::files::copy_files_to_clipboard,
            commands::files::copy_files_to_dir,
            commands::files::reveal_in_file_manager,
            commands::files::rename_schematic,
            commands::blocks::entity_texture_status,
            commands::blocks::entity_texture_pixels,
            commands::state::load_ui_state,
            commands::state::save_ui_state,
            commands::setup::base_db_status,
            commands::setup::default_minecraft_dir,
            commands::setup::scan_minecraft,
            commands::setup::setup_add_versions,
            commands::setup::load_setup_state,
            commands::setup::save_setup_state,
            commands::app::app_version,
            commands::app::check_update,
            commands::app::open_url,
        ])
        .run(context);

    if let Err(e) = result {
        app::startup::report(&e.to_string(), &identifier);
    }
}
