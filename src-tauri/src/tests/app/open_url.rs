use super::*;

#[test]
fn https_and_http_are_allowed() {
    assert!(validate("https://github.com/heiheids/schematic-manager/releases/tag/v0.0.2-dev.7").is_ok());
    assert!(validate("http://space.bilibili.com/19428259").is_ok());
    // 大小写不敏感
    assert!(validate("HTTPS://example.com/x").is_ok());
}

/// 别的协议会启动本机程序，一律不放行
#[test]
fn other_schemes_are_rejected() {
    for bad in [
        "file:///C:/Windows/System32/cmd.exe",
        "javascript:alert(1)",
        "ftp://example.com",
        "ms-settings:",
        "//example.com",
        "example.com",
        "",
    ] {
        assert!(validate(bad).is_err(), "{bad} 不该被放行");
    }
}

/// 能干扰命令行解析的字符必须挡掉：这些不是「可疑」，是可以直接改变
/// 命令语义的字符
#[test]
fn shell_metacharacters_are_rejected() {
    for bad in [
        "https://x/\"&calc&\"",
        "https://x/%PATH%",
        "https://x/a b",
        "https://x/a\nb",
        "https://x/a|b",
        "https://x/a^b",
        "https://x/a<b>c",
        "https://x/a;b",
        "https://x/a`b`",
        "https://x/a\\b",
        "https://x/a$b",
        "https://x/a'b'",
        "https://x/a(b)",
    ] {
        assert!(validate(bad).is_err(), "{bad} 含有必须拒绝的字符");
    }
}

#[test]
fn overlong_url_is_rejected() {
    let long = format!("https://example.com/{}", "a".repeat(MAX_URL_LEN));
    assert!(validate(&long).is_err());
}

/// Windows 分支要给出 start 的占位标题，否则带引号的目标会被当成标题；
/// 其余平台直接把 URL 作为参数。
#[test]
fn command_arguments_are_well_formed() {
    let url = "https://example.com/a";
    let cmd = command_for(url);
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    #[cfg(target_os = "windows")]
    {
        assert_eq!(cmd.get_program().to_string_lossy(), "cmd");
        assert_eq!(args, vec!["/C", "start", "", url]);
    }
    #[cfg(target_os = "macos")]
    {
        assert_eq!(cmd.get_program().to_string_lossy(), "open");
        assert_eq!(args, vec![url]);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        assert_eq!(cmd.get_program().to_string_lossy(), "xdg-open");
        assert_eq!(args, vec![url]);
    }
}

/// 校验不通过时不能真的去启动进程——这是「先校验后执行」的关键：
/// 若顺序写反，非法链接在报错前就已经被系统打开了。
#[test]
fn open_refuses_invalid_before_spawning() {
    assert!(open("file:///C:/Windows/System32/calc.exe").is_err());
    assert!(open("https://x/%PATH%").is_err());
}
