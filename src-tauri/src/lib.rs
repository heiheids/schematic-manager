mod backup;
mod base_db;
mod block_icon;
mod block_model;
mod block_search;
mod blockstate;
mod blockstate_chain;
mod code_models;
mod blockdb;
mod blockdb_write;
mod commands;
mod convert;
mod data_dir;
mod dto;
mod duplicate;
mod entitydb;
mod entitydb_write;
mod error;
mod file_ops;
mod geometry;
mod jar_file;
mod jar_index;
mod litematic;
mod mc_download;
mod mcver;
mod mesh;
mod model_pack;
mod nbt;
mod open_url;
mod png;
mod pool;
mod pack_build;
mod replace;
// 针对任意真实投影集合的端到端核对，按需运行（标了 #[ignore]）
#[cfg(test)]
mod real_project;
mod run_log;
mod scene;
mod tint;
mod startup;
mod transform;
mod update;
mod visibility;
mod zip;

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
    startup::install_panic_hook(identifier.clone());

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
                Some(dir) => match run_log::open(&dir) {
                    Ok(()) => {
                        // 等级先按界面上次存下的取值，debug 用户才能从第一行起
                        // 就有完整日志，而不是等界面把等级推过来才开始记
                        run_log::set_level(commands::saved_log_level(app.handle()));
                        let info = app.package_info();
                        run_log::info(&format!(
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
                        startup::show_message(&text);
                    }
                },
                None => {
                    let text = "运行日志无法启用，本次运行的日志不会写入：定位应用数据目录失败。";
                    eprintln!("{text}");
                    startup::show_message(text);
                }
            }

            // 应用数据被读写前最早的时机：窗口已由 Tauri 建好（webview 的数据
            // 目录那时也建了），但前端命令要等事件循环才开始，界面状态与方块库
            // 都还没被碰过
            if let Err(e) = data_dir::migrate_legacy_data(app.handle()) {
                // 搬迁失败会让用户以为「更新完东西全没了」，先记下来再向上报
                run_log::error(&format!("应用数据目录搬迁失败：{e}"));
                return Err(e.into());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::peek_schematic,
            commands::scan_litematics,
            commands::analyze_schematic,
            commands::render_block_icons,
            commands::mc_version_name,
            commands::build_scene,
            commands::list_backups,
            commands::restore_backup,
            commands::delete_backup,
            commands::configure_run_log,
            commands::read_run_log,
            commands::clear_run_log,
            commands::open_run_log_dir,
            commands::log_ui_event,
            commands::run_log_dir,
            commands::search_blocks,
            commands::list_block_versions,
            commands::preview_replace,
            commands::apply_replace,
            commands::scan_duplicates,
            commands::convert_targets,
            commands::analyze_conversion,
            commands::apply_conversion,
            commands::save_metadata,
            commands::delete_files,
            commands::copy_files_to_clipboard,
            commands::copy_files_to_dir,
            commands::reveal_in_file_manager,
            commands::rename_schematic,
            commands::entity_texture_status,
            commands::entity_texture_pixels,
            commands::load_ui_state,
            commands::save_ui_state,
            commands::base_db_status,
            commands::default_minecraft_dir,
            commands::scan_minecraft,
            commands::setup_add_versions,
            commands::load_setup_state,
            commands::save_setup_state,
            commands::app_version,
            commands::check_update,
            commands::open_url,
        ])
        .run(context);

    if let Err(e) = result {
        startup::report(&e.to_string(), &identifier);
    }
}
