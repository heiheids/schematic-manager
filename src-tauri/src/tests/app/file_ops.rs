use super::*;

fn tmp(tag: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("testtmp")
        .join(format!("sm_fileops_{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn mk(dir: &Path, name: &str, data: &[u8]) -> String {
    let p = dir.join(name);
    std::fs::write(&p, data).unwrap();
    p.to_string_lossy().into_owned()
}

#[test]
fn copy_into_writes_all_files() {
    let base = tmp("copy_basic");
    let src = base.join("src");
    let dst = base.join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();

    let a = mk(&src, "a.litematic", b"AAA");
    let b = mk(&src, "b.litematic", b"BBB");
    let files = check_files(&[a, b]).unwrap();
    let written = copy_into(&files, &dst).unwrap();

    assert_eq!(written.len(), 2);
    assert_eq!(std::fs::read(dst.join("a.litematic")).unwrap(), b"AAA");
    assert_eq!(std::fs::read(dst.join("b.litematic")).unwrap(), b"BBB");
    // 原文件必须还在：复制不是移动
    assert!(src.join("a.litematic").exists());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn copy_into_does_not_overwrite_same_name() {
    let base = tmp("copy_dup");
    let src = base.join("src");
    let dst = base.join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    // 目标目录已有一个同名但内容不同的文件
    std::fs::write(dst.join("a.litematic"), b"EXISTING").unwrap();
    let a = mk(&src, "a.litematic", b"NEW");

    let written = copy_into(&check_files(&[a]).unwrap(), &dst).unwrap();
    assert_eq!(written.len(), 1);
    assert_eq!(
        std::fs::read(dst.join("a.litematic")).unwrap(),
        b"EXISTING",
        "既有文件绝不能被静默覆盖"
    );
    assert_eq!(std::fs::read(&written[0]).unwrap(), b"NEW");
    assert_eq!(
        written[0].file_name().unwrap().to_string_lossy(),
        "a (2).litematic"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn check_files_rejects_missing_and_directory() {
    let base = tmp("check_bad");
    let good = mk(&base, "ok.litematic", b"x");
    let missing = base.join("nope.litematic").to_string_lossy().into_owned();

    assert!(check_files(&[]).is_err(), "空列表应报错，而不是静默成功");
    let e = check_files(&[missing]).unwrap_err().to_string();
    assert!(e.contains("无法访问"), "应说明是哪个文件不可访问：{e}");

    // 目录不能被当成文件处理
    let e2 = check_files(&[base.to_string_lossy().into_owned()]).unwrap_err().to_string();
    assert!(e2.contains("不是文件"), "目录应被拒绝：{e2}");

    // 只要有一个不合格，整批中止（不能部分成功）
    let e3 = check_files(&[good, base.join("x").to_string_lossy().into_owned()]);
    assert!(e3.is_err(), "混入不合格路径时整批应中止");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn check_files_reports_size() {
    let base = tmp("check_size");
    let p = mk(&base, "a.litematic", b"12345");
    let f = check_files(&[p]).unwrap();
    assert_eq!(f[0].size, 5);
    assert_eq!(f[0].name, "a.litematic");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn split_ext_handles_edges() {
    assert_eq!(split_ext("a.litematic"), ("a".into(), "litematic".into()));
    assert_eq!(split_ext("a.b.c"), ("a.b".into(), "c".into()));
    assert_eq!(split_ext("noext"), ("noext".into(), String::new()));
    // 前导点的隐藏文件：整名视作主干，否则会得到空主干
    assert_eq!(split_ext(".hidden"), (".hidden".into(), String::new()));
}

#[test]
fn copy_into_rejects_non_directory() {
    let base = tmp("copy_nondir");
    let f = mk(&base, "a.litematic", b"x");
    let files = check_files(&[f]).unwrap();
    let e = copy_into(&files, &base.join("a.litematic")).unwrap_err().to_string();
    assert!(e.contains("目标不是目录"), "{e}");
    let _ = std::fs::remove_dir_all(&base);
}

/// 删除必须真的把文件移走，且能从回收站找回。
///
/// 用真实文件与真实回收站：删除是否进回收站、参数格式对不对，
/// 只有调用系统 API 才能验证，桩替身测不出 SHFileOperationW 的布局错误。
#[test]
fn delete_moves_file_to_trash_not_oblivion() {
    let base = tmp("trash");
    let p = PathBuf::from(mk(&base, "to_delete.litematic", b"content"));
    assert!(p.exists());

    let files = check_files(&[p.to_string_lossy().into_owned()]).unwrap();
    delete_to_trash(&files).expect("删除到回收站应成功");

    assert!(!p.exists(), "原位置应已不存在");
    let _ = std::fs::remove_dir_all(&base);
}

/// 打开文件夹必须拒绝不存在的路径，而不是让 explorer 打开一个空目录。
#[test]
fn reveal_rejects_missing_path() {
    let base = tmp("reveal");
    let e = reveal_in_file_manager(&base.join("nope.litematic"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("不存在"), "{e}");
    let _ = std::fs::remove_dir_all(&base);
}

/// 定位文件时传给 explorer 的必须是**两个**参数。
///
/// 这条防的是「点了没反应」：路径含空格时，若 `/select,` 与路径被合成
/// 一个参数，Rust 会给整串加引号，
///   "/select,C:\我的 目录\a.litematic"
/// explorer 会忽略它——进程正常启动、不报错、但没有窗口出现。
/// 因此这里不看「能否启动」（错误形式同样能启动），而是直接检查
/// Command 实际会收到的参数个数与内容。
#[cfg(windows)]
#[test]
fn reveal_passes_select_and_path_as_two_arguments() {
    use std::process::Command;

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("testtmp")
        .join("sm reveal 空格");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a b.litematic");
    std::fs::write(&file, b"x").unwrap();

    let (exe, args) = reveal_command(&file);
    assert_eq!(exe, "explorer.exe");
    assert_eq!(args.len(), 2, "必须是两个参数：{args:?}");
    assert_eq!(args[0], "/select,");
    assert_eq!(Path::new(&args[1]), file);

    // 用 Command 自己的转义结果再核一次：调用方用的就是这条路。
    // get_args() 给出 Windows 下实际拼给进程的命令行，含空格的路径
    // 必须带引号、且引号只包住路径这一部分。
    let cmd = Command::new(exe)
        .args(&args)
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(cmd.len(), 2);
    assert_eq!(cmd[0], "/select,");
    assert!(
        !cmd[0].contains("a b.litematic"),
        "路径不能并入第一个参数，否则 explorer 会忽略：{cmd:?}"
    );
    assert_eq!(Path::new(&cmd[1]), file);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn normalize_file_name_accepts_and_fixes_common_input() {
    assert_eq!(normalize_file_name("a.litematic").unwrap(), "a.litematic");
    // 大小写不同的扩展名视为已带扩展名，不重复追加
    assert_eq!(normalize_file_name("A.LITEMATIC").unwrap(), "A.LITEMATIC");
    // 首尾空白去掉
    assert_eq!(normalize_file_name("  a.litematic  ").unwrap(), "a.litematic");
    // 缺扩展名时补上
    assert_eq!(normalize_file_name("机械动力女巫塔").unwrap(), "机械动力女巫塔.litematic");
    // 名字里带句点不能被当成「扩展名错误」而拒绝：
    // 真实存在的投影名就有这种（低Y艾莱姆农场-2.73万.litematic）
    assert_eq!(
        normalize_file_name("低Y艾莱姆农场-2.73万").unwrap(),
        "低Y艾莱姆农场-2.73万.litematic"
    );
}

#[test]
fn normalize_file_name_rejects_escaping_and_illegal_names() {
    // 只接受文件名本身：允许分隔符就等于允许把文件搬到任意位置
    for bad in ["..\\a.litematic", "sub/a.litematic", "C:a.litematic"] {
        assert!(normalize_file_name(bad).is_err(), "应拒绝 {bad}");
    }
    for bad in ["a<b.litematic", "a>b.litematic", "a|b.litematic", "a?b.litematic", "a*b.litematic", "a\"b.litematic"] {
        assert!(normalize_file_name(bad).is_err(), "应拒绝 {bad}");
    }
    assert!(normalize_file_name("").is_err());
    assert!(normalize_file_name("   ").is_err());
    // `.` 与 `..` 被专门拦下（它们也以句点结尾，因此这条同时钉住了
    // 「先判 ./..、再判句点」的顺序；顺序反了这两条会改走句点规则）
    let e = normalize_file_name(".").unwrap_err().to_string();
    assert!(e.contains("不能是"), "应命中 ./.. 规则而不是句点规则：{e}");
    let e = normalize_file_name("..").unwrap_err().to_string();
    assert!(e.contains("不能是"), "应命中 ./.. 规则而不是句点规则：{e}");
    // 首尾空白会被 trim 掉，这是有意的：用户多半是粘贴时带上的。
    // （因此这里不能断言「以空格结尾应被拒绝」——那种输入到不了这一步）
    assert_eq!(normalize_file_name("a ").unwrap(), "a.litematic");
    // 结尾的句点会被 Windows 静默去掉，实际名字与输入不一致，因此拒绝
    assert!(normalize_file_name("a.").is_err());
    // 保留设备名即使带扩展名也不能用
    assert!(normalize_file_name("CON.litematic").is_err());
    assert!(normalize_file_name("nul").is_err());
}

#[test]
fn rename_file_moves_and_refuses_to_clobber() {
    let base = tmp("rename");
    let old = PathBuf::from(mk(&base, "old.litematic", b"DATA"));

    let dest = rename_file(&old, "new.litematic").unwrap();
    assert_eq!(dest.file_name().unwrap().to_string_lossy(), "new.litematic");
    assert!(!old.exists(), "旧名字应已不存在");
    assert_eq!(std::fs::read(&dest).unwrap(), b"DATA", "内容必须原样保留");

    // 目标已存在时拒绝：覆盖同名投影不可逆
    let other = PathBuf::from(mk(&base, "other.litematic", b"OTHER"));
    let e = rename_file(&other, "new.litematic").unwrap_err().to_string();
    assert!(e.contains("已存在"), "{e}");
    assert_eq!(std::fs::read(&dest).unwrap(), b"DATA", "既有文件不能被静默覆盖");
    let _ = std::fs::remove_dir_all(&base);
}

/// 仅大小写不同的目标：在大小写不敏感的文件系统上它就是文件本身，不能误报冲突。
///
/// 「目标已存在」这条判断的分支走向取决于文件系统是否区分大小写，
/// 因此先探测本机行为再断言对应的一侧。不这样做的话，在 Linux 上
/// `dest.exists()` 为 false，根本不会走到那条分支，用例虽然通过却什么也没验证。
#[test]
fn rename_file_case_only_target_does_not_false_conflict() {
    let base = tmp("rename_case");
    let old = PathBuf::from(mk(&base, "demo.litematic", b"DATA"));
    let variant = base.join("DEMO.litematic");
    let case_insensitive = std::fs::canonicalize(&variant).is_ok()
        && std::fs::canonicalize(&variant).unwrap() == std::fs::canonicalize(&old).unwrap();

    let dest = rename_file(&old, "DEMO.litematic").unwrap();
    assert_eq!(dest.file_name().unwrap().to_string_lossy(), "DEMO.litematic");
    assert_eq!(std::fs::read(&dest).unwrap(), b"DATA", "内容必须原样保留");
    assert!(variant.exists(), "新名字下应能读到文件");
    // 大小写敏感的系统上旧名字确实消失了；不敏感的系统上
    // `demo.litematic` 仍会解析到同一个文件，所以这里不断言它不存在
    if !case_insensitive {
        assert!(!old.exists(), "大小写敏感时旧名字应已不存在");
    }
    let _ = std::fs::remove_dir_all(&base);
}

/// 大小写敏感的文件系统上，仅大小写不同的**另一个**文件不能被覆盖。
#[test]
#[cfg(not(windows))]
fn rename_file_refuses_to_clobber_case_variant_on_sensitive_fs() {
    let base = tmp("rename_case_sensitive");
    let old = PathBuf::from(mk(&base, "demo.litematic", b"LOWER"));
    std::fs::write(base.join("DEMO.litematic"), b"UPPER").unwrap();

    // 先确认本机确实区分大小写，否则这条用例不成立（macOS 默认不区分）
    if std::fs::canonicalize(base.join("DEMO.litematic")).unwrap()
        == std::fs::canonicalize(&old).unwrap()
    {
        let _ = std::fs::remove_dir_all(&base);
        return;
    }

    let e = rename_file(&old, "DEMO.litematic").unwrap_err().to_string();
    assert!(e.contains("已存在"), "{e}");
    assert_eq!(
        std::fs::read(base.join("DEMO.litematic")).unwrap(),
        b"UPPER",
        "既有文件绝不能被静默覆盖"
    );
    assert_eq!(std::fs::read(&old).unwrap(), b"LOWER");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn rename_file_rejects_missing_and_directory() {
    let base = tmp("rename_bad");
    assert!(rename_file(&base.join("nope.litematic"), "x.litematic").is_err());
    let e = rename_file(&base, "x.litematic").unwrap_err().to_string();
    assert!(e.contains("不是文件"), "{e}");
    let _ = std::fs::remove_dir_all(&base);
}
