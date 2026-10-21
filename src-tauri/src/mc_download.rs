//! 从官方 API 或 BMCL 镜像下载 Minecraft 客户端 jar。
//!
//! 官方接口分两步：先读版本清单拿到该版本 version.json 的地址，
//! 再从 version.json 里取客户端 jar 的下载地址。BMCL 镜像的客户端
//! 地址可以直接拼出来，不用发请求。
//! 清单/版本 JSON 的解析与「从哪个地址下载」是纯函数，便于离线测试；
//! 真正发请求的只有 [`fetch_manifest`] 与 [`download_to`]。

use crate::error::{AppError, AppResult};
use std::io::Read;
use std::path::Path;
use std::time::Duration;

/// 官方版本清单（manifest v2）地址
pub const OFFICIAL_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// BMCL 镜像的客户端 jar 地址前缀（`{前缀}/{版本 id}/client`）
pub const BMCL_CLIENT_BASE: &str = "https://bmclapi2.bangbang93.com/version";

/// 请求超时要给足：客户端 jar 有 20~30 MB，镜像站在高峰时段并不快。
/// 没有这个上限时，一个挂起的连接会让「版本库」界面永远停在转圈。
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(300))
        .build()
}

/// 把网络错误翻译成带上下文的 AppError。
///
/// ureq 的错误文本本身不含 URL，直接向上抛会让用户分不清
/// 是清单没拉到还是 jar 没下到。
fn http_err(url: &str, e: ureq::Error) -> AppError {
    AppError::Parse(format!("下载失败 {url}：{e}"))
}

/// 拉取官方版本清单全文（几 MB 的 JSON，直接按文本处理）
pub fn fetch_manifest() -> AppResult<String> {
    fetch_text(OFFICIAL_MANIFEST_URL)
}

/// 从官方版本清单里找某个版本的 version.json 地址。
///
/// 清单里没有该版本时报错而不是返回空：那意味着这个 id 根本不存在
/// （或不是正式版），静默跳过会让用户以为下载成功了。
pub fn version_json_url(manifest: &str, id: &str) -> AppResult<String> {
    let doc: serde_json::Value =
        serde_json::from_str(manifest).map_err(|e| AppError::Parse(format!("版本清单解析失败：{e}")))?;
    let versions = doc
        .get("versions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AppError::Parse("版本清单缺少 versions 数组".into()))?;
    for v in versions {
        if v.get("id").and_then(|x| x.as_str()) == Some(id) {
            return v
                .get("url")
                .and_then(|x| x.as_str())
                .map(str::to_owned)
                .ok_or_else(|| AppError::Parse(format!("版本 {id} 的条目缺少 url")));
        }
    }
    Err(AppError::Parse(format!(
        "官方版本清单里没有 {id}（可能不是正式版）"
    )))
}

/// 从 version.json 里取客户端 jar 的下载地址（downloads.client.url）
pub fn client_url_from_version_json(json: &str) -> AppResult<String> {
    let doc: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| AppError::Parse(format!("version.json 解析失败：{e}")))?;
    doc.get("downloads")
        .and_then(|d| d.get("client"))
        .and_then(|c| c.get("url"))
        .and_then(|u| u.as_str())
        .map(str::to_owned)
        .ok_or_else(|| AppError::Parse("version.json 里没有 downloads.client.url".into()))
}

/// 官方接口里某个版本客户端 jar 的下载地址：清单 → version.json → client.url
pub fn official_client_url(manifest: &str, id: &str) -> AppResult<String> {
    let json_url = version_json_url(manifest, id)?;
    let body = fetch_text(&json_url)?;
    client_url_from_version_json(&body)
}

/// BMCL 镜像的客户端 jar 地址，直接按 id 拼接，不需要请求
pub fn bmcl_client_url(id: &str) -> String {
    format!("{BMCL_CLIENT_BASE}/{id}/client")
}

/// 把 URL 的内容下载到本地文件，过程中通过回调汇报进度。
///
/// `on_progress(已收字节, 总字节)`：总字节为 0 表示服务器没给长度。
/// 落盘细节见 [`write_body`]。
pub fn download_to(
    url: &str,
    dest: &Path,
    on_progress: &mut dyn FnMut(u64, u64),
) -> AppResult<()> {
    let resp = agent()
        .get(url)
        .call()
        .map_err(|e| http_err(url, e))?;
    let status = resp.status();
    let total: u64 = resp
        .header("Content-Length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    crate::run_log::debug(&format!("GET {url} -> {status}，开始下载，共 {total} 字节"));
    let mut reader = resp.into_reader();
    write_body(&mut reader, dest, total, on_progress)
}

/// 把响应正文写进目标文件：先写 `.tmp`，读完才改名。
///
/// 读到一半连接断掉时 ureq 会以「正文提前结束」报错，此时必须清掉
/// 半截文件：留在目标路径上会被后续流程当成「已下载」直接复用，
/// 于是每次构建都失败，而用户看不出是下载坏了。
///
/// 正文来源做成 `Read` 参数而不是在这里发请求：落盘行为（改名、失败
/// 清理）是这段代码的实质，可以脱离网络直接验证。
fn write_body(
    reader: &mut dyn Read,
    dest: &Path,
    total: u64,
    on_progress: &mut dyn FnMut(u64, u64),
) -> AppResult<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = dest.to_path_buf();
    tmp.as_mut_os_string().push(".tmp");

    let mut read_all = |tmp: &Path| -> AppResult<()> {
        let mut out = std::fs::File::create(tmp)?;
        let mut buf = [0u8; 64 * 1024];
        let mut received: u64 = 0;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            std::io::Write::write_all(&mut out, &buf[..n])?;
            received += n as u64;
            on_progress(received, total);
        }
        Ok(())
    };

    if let Err(e) = read_all(&tmp) {
        // 清掉半截文件；删不掉也不掩盖原始错误，原样把它报上去
        if let Err(rm) = std::fs::remove_file(&tmp) {
            crate::run_log::warn(&format!("清理未完成的下载 {tmp:?} 失败：{rm}"));
        }
        return Err(e);
    }
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

/// 语言表等资源文件的来源：与下载客户端 jar 的来源保持一致，
/// 用户选了镜像就跟着镜像，免得明明走的是镜像却又要去连官方。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetSource {
    Official,
    Bmcl,
}

/// 官方资源对象地址前缀（`{前缀}/{hash 前两位}/{hash}`）
pub const OFFICIAL_ASSET_BASE: &str = "https://resources.download.minecraft.net";

/// BMCL 镜像的资源对象地址前缀
pub const BMCL_ASSET_BASE: &str = "https://bmclapi2.bangbang93.com/assets";

/// 下载语言表需要的信息。
///
/// 打包成结构体而不是一串参数：这些字段是一件事的各个侧面（哪个版本、从哪来、
/// 本机有没有现成的），调用处传错顺序时编译器也帮不上忙。
pub struct LangDownload<'a> {
    /// 版本 id，用来找本机的 version.json、以及构造镜像的 version.json 地址
    pub version_id: &'a str,
    /// 与客户端 jar 相同的来源
    pub source: AssetSource,
    /// 本机 .minecraft：装了该版本时可直接读它的 version.json，省一次联网
    pub mc_dir: Option<&'a Path>,
    /// 官方版本清单全文（来源为官方、且需要联网取 version.json 时用）
    pub manifest: Option<&'a str>,
    /// 需要补齐的语言键（如 `zh_cn`）
    pub langs: &'a [&'a str],
}

/// 拉取一个文本资源（版本清单、version.json、资源索引都是 JSON 文本）
pub fn fetch_text(url: &str) -> AppResult<String> {
    let resp = agent().get(url).call().map_err(|e| http_err(url, e))?;
    let status = resp.status();
    let text = resp
        .into_string()
        .map_err(|e| AppError::Parse(format!("读取 {url} 失败：{e}")))?;
    // debug 等级记请求与响应：用户报「下载失败」「拿不到中文名」时，
    // 这两行就能看出请求打到哪、对方回了什么
    if crate::run_log::enabled(crate::run_log::Level::Debug) {
        crate::run_log::debug(&format!(
            "GET {url} -> {status}\n{}",
            crate::run_log::clip_body(&text)
        ));
    }
    Ok(text)
}

/// 拉取一个小的二进制资源（语言表约几百 KB，不需要落盘）
pub fn fetch_bytes(url: &str) -> AppResult<Vec<u8>> {
    let resp = agent().get(url).call().map_err(|e| http_err(url, e))?;
    let status = resp.status();
    let mut out = Vec::new();
    resp.into_reader()
        .read_to_end(&mut out)
        .map_err(|e| AppError::Parse(format!("读取 {url} 失败：{e}")))?;
    // 二进制正文不写进日志，只记长度：语言表这类内容写进去既不可读，
    // 也会把日志文件迅速撑到滚动上限
    crate::run_log::debug(&format!("GET {url} -> {status}，{} 字节", out.len()));
    Ok(out)
}

/// 从 version.json 里取资源索引地址（`assetIndex.url`）。
///
/// 索引里记录着「资源路径 -> 内容 hash」，语言表本身要靠它才能定位。
pub fn asset_index_url_from_version_json(json: &str) -> AppResult<String> {
    let doc: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| AppError::Parse(format!("version.json 解析失败：{e}")))?;
    doc.get("assetIndex")
        .and_then(|a| a.get("url"))
        .and_then(|u| u.as_str())
        .map(str::to_owned)
        .ok_or_else(|| AppError::Parse("version.json 里没有 assetIndex.url".into()))
}

/// 从资源索引里取某个语言表的对象 hash；索引里没有这个语言时给 `None`。
///
/// 「索引里没有」与「索引坏了」必须分开：前者说明这个版本确实不带该语言，
/// 后者说明数据不可信，混在一起会让坏索引被当成「没有中文」。
pub fn lang_object_hash(index_json: &str, lang: &str) -> AppResult<Option<String>> {
    let doc: serde_json::Value = serde_json::from_str(index_json)
        .map_err(|e| AppError::Parse(format!("资源索引解析失败：{e}")))?;
    Ok(doc
        .get("objects")
        .and_then(|o| o.get(format!("minecraft/lang/{lang}.json")))
        .and_then(|v| v.get("hash"))
        .and_then(|h| h.as_str())
        .map(str::to_owned))
}

/// 资源对象的下载地址：`{前缀}/{hash 前两位}/{hash}`。
///
/// hash 短于两位时直接报错：那是索引被截断的样子，拼出一个必然 404 的地址
/// 只会让用户看到「下载失败」，而看不出根因在索引。
pub fn asset_object_url(base: &str, hash: &str) -> AppResult<String> {
    if hash.len() < 2 {
        return Err(AppError::Parse(format!("资源 hash 不合法：{hash}")));
    }
    Ok(format!("{base}/{}/{}", &hash[..2], hash))
}

/// 从资源索引下载指定的语言表，返回 `(包内路径, 内容)`。
///
/// 客户端 jar 里只有 `en_us.json`（官方从 1.6 起把多语言放进了资源库），
/// 所以中文只能这样取。取不到时如实报错，由调用方决定是继续构建
/// （名称回落到英文）还是提示用户——不能悄悄当成「这个方块没有中文名」。
pub fn download_lang_files(req: &LangDownload<'_>) -> AppResult<Vec<(String, Vec<u8>)>> {
    let version_json = read_version_json(req)?;
    let index_url = asset_index_url_from_version_json(&version_json)?;
    let index = fetch_text(&index_url)?;
    let base = match req.source {
        AssetSource::Official => OFFICIAL_ASSET_BASE,
        AssetSource::Bmcl => BMCL_ASSET_BASE,
    };
    let mut out = Vec::new();
    for lang in req.langs {
        let Some(hash) = lang_object_hash(&index, *lang)? else {
            continue;
        };
        let url = asset_object_url(base, &hash)?;
        out.push((
            format!("assets/minecraft/lang/{lang}.json"),
            fetch_bytes(&url)?,
        ));
    }
    Ok(out)
}

/// 取该版本的 version.json：优先用本机装好的那份，没有再联网。
///
/// 本机那份是离线可用的，而且装了该版本时它一定与 jar 同源；联网那份
/// 官方走清单、镜像走 `/version/{id}/json`（镜像的 version.json 是官方原文，
/// 因此 assetIndex.url 仍指向官方地址）。
fn read_version_json(req: &LangDownload<'_>) -> AppResult<String> {
    if let Some(dir) = req.mc_dir {
        let path = dir
            .join("versions")
            .join(req.version_id)
            .join(format!("{}.json", req.version_id));
        if let Ok(text) = std::fs::read_to_string(&path) {
            return Ok(text);
        }
    }
    match (req.source, req.manifest) {
        (AssetSource::Official, Some(manifest)) => {
            let url = version_json_url(manifest, req.version_id)?;
            fetch_text(&url)
        }
        _ => fetch_text(&format!("{BMCL_CLIENT_BASE}/{}/json", req.version_id)),
    }
}

#[cfg(test)]
mod tests {
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
        crate::run_log::open(&dir).expect("应能打开运行日志");
        crate::run_log::configure(crate::run_log::Level::Debug, "2026-10-19")
            .expect("应能配置运行日志");

        let manifest = fetch_manifest().expect("应能取到版本清单");
        assert!(manifest.contains("versions"), "清单应是 JSON");

        let lines = crate::run_log::read(50);
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
}
