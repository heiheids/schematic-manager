use super::*;

/// 补语言表时只请求 jar 里没有的那些。
///
/// 资源索引里没有 `minecraft/lang/en_us.json`（官方把它留在客户端 jar 里），
/// 若把 `en_us` 也算作「本地缺、要去下」，库完全正常时也会报「未取到中文
/// 语言表」，用户会以为中文名没生效。
#[test]
fn only_langs_absent_from_the_jar_are_downloaded() {
    // 本机 .minecraft/assets 为空（从没启动过游戏）——这是最常见的场景
    assert_eq!(langs_to_download(&[]), vec!["zh_cn"]);

    // 本机已经有中文表（用户启动过游戏）时什么都不用下
    let local = vec![(
        "assets/minecraft/lang/zh_cn.json".to_string(),
        b"{}".to_vec(),
    )];
    assert!(langs_to_download(&local).is_empty());

    // 只有英文表（客户端 jar 里那份）不算缺中文
    let en_only = vec![(
        "assets/minecraft/lang/en_us.json".to_string(),
        b"{}".to_vec(),
    )];
    assert_eq!(langs_to_download(&en_only), vec!["zh_cn"]);
}
