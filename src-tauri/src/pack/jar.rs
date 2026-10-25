//! 以 jar/zip 文件为内容来源，供资源包构建使用。
//!
//! 把 `pack::build::JarSource` 落到真实文件上：打开 zip、读中央目录，
//! 按需解压条目。PNG 贴图也在这里解码成像素供构建使用。
//!
//! 与 pack::build 分开是为了让构建逻辑不依赖文件系统：构建侧只认
//! JarSource 契约，因此可用内存假 jar 完整测试；这里只负责把文件接上。

use crate::error::{AppError, AppResult};
use crate::pack::build::JarSource;
use crate::format::zip::Archive;
use std::collections::HashMap;
use std::path::Path;

/// 一个已打开的 jar 文件。
///
/// 整个文件的字节常驻内存：Java 的 jar 通常几十 MB，而构建过程会反复
/// 读取条目（每个 blockstate 与其父模型链）。逐条 seek 读盘在机械盘上
/// 会明显拖慢导入，一次性读入更简单也更快。
#[derive(Debug)]
pub struct JarFile {
    bytes: Vec<u8>,
    archive: Archive,
    /// 条目名 -> 内容，已读取过的缓存
    cache: HashMap<String, Vec<u8>>,
    /// jar 之外的语言文件：路径 -> 内容
    ///
    /// 官方客户端 jar 只带 `en_us.json`，中文等语言随资源索引下载后存在
    /// `.minecraft/assets/objects` 里，构建基础库时要一并喂进来，
    /// 否则方块名整列没有中文。
    extra_langs: HashMap<String, Vec<u8>>,
}

impl JarFile {
    /// 打开一个 jar/zip 文件
    pub fn open(path: &str) -> AppResult<Self> {
        Self::open_with_langs(path, Vec::new())
    }

    /// 打开一个 jar 并附带 jar 之外的语言文件（路径 -> 内容）
    pub fn open_with_langs(
        path: &str,
        extra_langs: Vec<(String, Vec<u8>)>,
    ) -> AppResult<Self> {
        let bytes = std::fs::read(path).map_err(|e| {
            AppError::Parse(format!("无法读取 {path}：{e}"))
        })?;
        if bytes.len() < 22 {
            return Err(AppError::Parse(format!(
                "{path} 过小，不像是 jar/zip 文件"
            )));
        }
        let archive = crate::format::zip::open(&bytes)?;
        Ok(Self {
            bytes,
            archive,
            cache: HashMap::new(),
            extra_langs: extra_langs.into_iter().collect(),
        })
    }

    /// 条目的原始（未解压）内容
    pub fn raw(&self, name: &str) -> AppResult<Vec<u8>> {
        let e = self
            .archive
            .find(name)
            .ok_or_else(|| AppError::Parse(format!("jar 里没有条目 {name}")))?;
        crate::format::zip::read(&self.bytes, e)
    }

    /// 把 PNG 贴图解码成 (宽, 高, RGBA)
    pub fn decode_texture(&self, path: &str) -> Option<(u16, u16, Vec<u8>)> {
        let entry = self.texture_entry(path)?;
        let bytes = self.raw(&entry).ok()?;
        let img = crate::format::png::decode(&bytes).ok()?;
        let w = u16::try_from(img.width).ok()?;
        let h = u16::try_from(img.height).ok()?;
        if img.rgba.len() != w as usize * h as usize * 4 {
            return None;
        }
        Some((w, h, img.rgba))
    }

    /// 贴图路径（`ns:block/stone` 或 `block/stone`）对应的条目名
    fn texture_entry(&self, path: &str) -> Option<String> {
        let (ns, p) = match path.split_once(':') {
            Some((n, p)) => (n, p),
            // 没有命名空间时按 minecraft 处理：模型里的贴图引用常省略前缀
            None => ("minecraft", path),
        };
        if ns.is_empty() || p.is_empty() {
            return None;
        }
        let exact = format!("assets/{ns}/textures/{p}.png");
        if self.archive.find(&exact).is_some() {
            return Some(exact);
        }
        // 少见的扩展名大小写差异
        let upper = format!("assets/{ns}/textures/{p}.PNG");
        if self.archive.find(&upper).is_some() {
            return Some(upper);
        }
        None
    }

}

impl JarSource for JarFile {
    fn entries(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .archive
            .entries
            .iter()
            .filter(|e| !e.is_dir())
            .map(|e| e.name.clone())
            .collect();
        // 排序使构建结果与 zip 条目顺序无关
        v.sort();
        v
    }

    fn extra_lang_paths(&self) -> Vec<String> {
        let mut v: Vec<String> = self.extra_langs.keys().cloned().collect();
        v.sort();
        v
    }

    fn read_extra_lang(&self, name: &str) -> Option<Vec<u8>> {
        self.extra_langs.get(name).cloned()
    }

    fn read(&self, name: &str) -> AppResult<Vec<u8>> {
        if let Some(v) = self.cache.get(name) {
            return Ok(v.clone());
        }
        self.raw(name)
    }
}

/// 同上，并附带 jar 之外的语言文件（路径 -> 内容）。
///
/// 官方客户端 jar 只带 `en_us.json`，中文在 `.minecraft/assets` 的对象库里；
/// 构建基础库时必须把中文喂进来，否则方块名整列没有中文。
pub fn build_pack_to_with_langs(
    jar_path: &str,
    out_path: &str,
    pack_name: &str,
    data_version: i32,
    is_overlay: bool,
    extra_langs: Vec<(String, Vec<u8>)>,
) -> AppResult<crate::pack::build::BuildStats> {
    let jar = JarFile::open_with_langs(jar_path, extra_langs)?;
    let built = crate::pack::build::build(
        &jar,
        pack_name,
        data_version,
        is_overlay,
        |path| jar.decode_texture(path),
    )?;

    if let Some(parent) = Path::new(out_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 资源包文件按 zlib 压缩（与基础库的单版本包一致），
    // 读取端 BlockDb::load 按魔数分派，两种容器都可读
    let packed = compress_zlib(&built.data)?;
    std::fs::write(out_path, &packed)?;
    Ok(built.stats)
}

/// zlib 压缩
fn compress_zlib(data: &[u8]) -> AppResult<Vec<u8>> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    e.write_all(data)?;
    Ok(e.finish()?)
}

#[cfg(test)]
#[path = "../tests/pack/jar.rs"]
mod tests;
