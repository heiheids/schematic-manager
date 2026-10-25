use super::*;
use std::path::PathBuf;

#[test]
fn version_json_url_finds_the_requested_version() {
    let manifest = r#"{
        "latest": {"release": "1.20.4", "snapshot": "24w01a"},
        "versions": [
            {"id": "1.20.4", "type": "release", "url": "https://x/v1204.json"},
            {"id": "1.20.3", "type": "release", "url": "https://x/v1203.json"}
        ]
    }"#;
    assert_eq!(
        version_json_url(manifest, "1.20.3").expect("应找到"),
        "https://x/v1203.json"
    );
}

#[test]
fn version_json_url_rejects_unknown_id() {
    let manifest = r#"{"versions": [{"id": "1.20.3", "url": "https://x/v.json"}]}"#;
    let e = version_json_url(manifest, "9.9.9").unwrap_err().to_string();
    assert!(e.contains("9.9.9"), "错误应指出是哪个 id 没找到：{e}");
}

#[test]
fn client_url_comes_from_downloads_section() {
    let json = r#"{
        "id": "1.20.4",
        "downloads": {
            "client": {"url": "https://x/client.jar", "size": 100},
            "server": {"url": "https://x/server.jar"}
        }
    }"#;
    assert_eq!(
        client_url_from_version_json(json).expect("应取到"),
        "https://x/client.jar"
    );

    // 服务端地址与客户端地址同层，取错键会下到 server jar——
    // 那玩意儿不是资源包能解析的东西，所以缺键必须报错而不是将就
    let no_client = r#"{"downloads": {"server": {"url": "https://x/server.jar"}}}"#;
    assert!(client_url_from_version_json(no_client).is_err());
}

/** 测试目录落在 target 下：部分受限环境不允许写系统临时目录。 */
fn test_dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join(format!(
            "mc_download_{tag}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&dir).expect("应能建立测试目录");
    dir
}

/// 读一半就断的正文：模拟连接被提前掐断（ureq 会在此处报错）
struct CutOff {
    left: usize,
}

impl std::io::Read for CutOff {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "正文提前结束",
            ));
        }
        let n = self.left.min(buf.len());
        buf[..n].fill(b'x');
        self.left -= n;
        Ok(n)
    }
}

#[test]
fn cut_off_body_leaves_nothing_behind() {
    let dir = test_dir("cutoff");
    let dest = dir.join("client.jar");
    let mut src = CutOff { left: 10 };

    let e = write_body(&mut src, &dest, 100, &mut |_, _| {}).unwrap_err();
    assert!(
        e.to_string().contains("提前结束"),
        "断流应如实报错：{e}"
    );
    assert!(
        !dest.exists(),
        "不完整的下载不能留在目标路径上——留下来就会被当成已下载并复用"
    );
    assert!(
        !dir.join("client.jar.tmp").exists(),
        "半截的临时文件也应清掉，避免堆在磁盘上"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn complete_body_is_renamed_into_place() {
    let dir = test_dir("complete");
    let dest = dir.join("client.jar");
    let mut src = std::io::Cursor::new(b"abcdef".to_vec());

    let mut seen: Vec<(u64, u64)> = Vec::new();
    write_body(&mut src, &dest, 6, &mut |r, t| seen.push((r, t))).expect("完整写入应成功");

    assert_eq!(std::fs::read(&dest).unwrap(), b"abcdef");
    assert!(
        seen.last().is_some_and(|&(r, t)| r == 6 && t == 6),
        "进度回调应报出收满：{seen:?}"
    );
    assert!(
        !dir.join("client.jar.tmp").exists(),
        "改名之后不该留下临时文件"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn asset_index_url_comes_from_asset_index_section() {
    let json = r#"{
        "id": "1.21.1",
        "assetIndex": {"id": "17", "url": "https://x/17.json", "sha1": "aa"},
        "downloads": {"client": {"url": "https://x/client.jar"}}
    }"#;
    assert_eq!(
        asset_index_url_from_version_json(json).expect("应取到索引地址"),
        "https://x/17.json"
    );

    // 缺 assetIndex 时必须报错：地址取不到就没法定位语言表，
    // 这时该由调用方明确降级，而不是拿去请求一个空地址
    let no_index = r#"{"downloads": {"client": {"url": "https://x/client.jar"}}}"#;
    let e = asset_index_url_from_version_json(no_index)
        .unwrap_err()
        .to_string();
    assert!(e.contains("assetIndex"), "错误要点明缺的是哪一段：{e}");
}

#[test]
fn lang_hash_is_looked_up_by_object_path() {
    let index = r#"{
        "objects": {
            "minecraft/lang/zh_cn.json": {"hash": "daf8df119618b33ad7476a6be4ac2eadc802fd93", "size": 448107},
            "minecraft/lang/en_us.json": {"hash": "aabbccddeeff00112233445566778899aabbccdd", "size": 1}
        }
    }"#;
    assert_eq!(
        lang_object_hash(index, "zh_cn").expect("索引应能解析").as_deref(),
        Some("daf8df119618b33ad7476a6be4ac2eadc802fd93")
    );
    assert_eq!(
        lang_object_hash(index, "ja_jp").expect("索引应能解析"),
        None,
        "索引里没有该语言时给 None，交给调用方跳过"
    );

    // 坏索引要报错而不是当作「没有这个语言」：后者会让人以为
    // 这个版本天生不带中文，从而不再排查数据问题
    assert!(lang_object_hash("{ 不是 JSON", "zh_cn").is_err());

    // 索引条目存在但缺 hash：同样按「没有」处理（结构不完整的条项目不可用）
    let broken_entry = r#"{"objects": {"minecraft/lang/zh_cn.json": {"size": 1}}}"#;
    assert_eq!(lang_object_hash(broken_entry, "zh_cn").unwrap(), None);
}

#[test]
fn asset_object_url_is_content_addressed() {
    let hash = "daf8df119618b33ad7476a6be4ac2eadc802fd93";
    assert_eq!(
        asset_object_url(OFFICIAL_ASSET_BASE, hash).unwrap(),
        format!("{OFFICIAL_ASSET_BASE}/da/{hash}")
    );
    assert_eq!(
        asset_object_url(BMCL_ASSET_BASE, hash).unwrap(),
        format!("{BMCL_ASSET_BASE}/da/{hash}")
    );

    // 被截断的 hash 会拼出一个必然 404 的地址，必须当场报错
    assert!(asset_object_url(OFFICIAL_ASSET_BASE, "d").is_err());
}

/// 联网验证整条链路：version.json → 资源索引 → 语言表对象。
///
/// 默认不跑（`cargo test -- --ignored` 手动执行）：单元测试不该依赖网络。
/// 它验的是三处 URL 拼装与解析合起来到底能不能取到真正的中文语言表——
/// 只测纯函数的话，「索引地址取自哪一段」「对象路径怎么拼」都可能各自正确
/// 而合起来取不到东西。
#[test]
#[ignore = "需要联网，手动运行 cargo test -- --ignored"]
fn downloads_chinese_lang_file_end_to_end() {
    let manifest = fetch_manifest().expect("应能取到版本清单");
    let req = LangDownload {
        version_id: "1.21.1",
        source: AssetSource::Official,
        mc_dir: None,
        manifest: Some(&manifest),
        langs: &["zh_cn"],
    };
    let got = download_lang_files(&req).expect("应能下载到中文语言表");
    assert_eq!(got.len(), 1, "应当只下到请求的那一个语言表");
    assert_eq!(got[0].0, "assets/minecraft/lang/zh_cn.json");

    let text = String::from_utf8_lossy(&got[0].1);
    assert!(
        text.contains("\"block.minecraft.stone\": \"石头\""),
        "取到的应当是中文语言表，实际开头是：{}",
        &text.chars().take(80).collect::<String>()
    );
}

/// 联网验证 debug 等级真的会把请求与响应写进运行日志。
///
/// 默认不跑（`cargo test -- --ignored`）。验的是「等级门控 + 网络层打点」
/// 合起来的效果：断言的请求行、状态码与响应正文片段都由真实网络产生，
/// 只测 `clip_body` 之类的纯函数证明不了这些行真的落进了文件。
#[test]
#[ignore = "需要联网，手动运行 cargo test -- --ignored"]
fn debug_level_records_requests_and_responses_in_the_run_log() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join("sm_run_log_network");
    let _ = std::fs::remove_dir_all(&dir);
    crate::app::run_log::open(&dir).expect("应能打开运行日志");
    crate::app::run_log::configure(crate::app::run_log::Level::Debug, "2026-10-19")
        .expect("应能配置运行日志");

    let manifest = fetch_manifest().expect("应能取到版本清单");
    assert!(manifest.contains("versions"), "清单应是 JSON");

    let lines = crate::app::run_log::read(50);
    assert!(
        lines.iter().any(|l| l.contains("GET ") && l.contains("200")),
        "debug 等级应记下请求行与状态码：{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("versions")),
        "debug 等级应记下响应正文片段：{lines:?}"
    );
    assert!(
        lines.iter().all(|l| !l.contains('\n')),
        "每条日志必须是一行，响应里的换行要被转义：{lines:?}"
    );
}
