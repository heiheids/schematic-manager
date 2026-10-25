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
    crate::app::run_log::debug(&format!("GET {url} -> {status}，开始下载，共 {total} 字节"));
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
            crate::app::run_log::warn(&format!("清理未完成的下载 {tmp:?} 失败：{rm}"));
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
    if crate::app::run_log::enabled(crate::app::run_log::Level::Debug) {
        crate::app::run_log::debug(&format!(
            "GET {url} -> {status}\n{}",
            crate::app::run_log::clip_body(&text)
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
    crate::app::run_log::debug(&format!("GET {url} -> {status}，{} 字节", out.len()));
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
#[path = "../tests/mc/download.rs"]
mod tests;
