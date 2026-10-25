use super::*;

/// 构造一个只含中央目录的最小 zip，便于精确控制字段
struct ZipBuilder {
    data: Vec<u8>,
    entries: Vec<(String, u32, u32, u32, u64, u16)>,
}

impl ZipBuilder {
    fn new() -> Self {
        Self { data: Vec::new(), entries: Vec::new() }
    }

    /// 直接塞入原始字节的条目（data 为已压缩内容）
    fn raw_entry(
        &mut self,
        name: &str,
        method: u16,
        raw: &[u8],
        uncompressed_size: u32,
        crc: u32,
    ) {
        let local_offset = self.data.len() as u64;
        // 本地文件头
        self.data.extend_from_slice(&LOC_SIG.to_le_bytes());
        self.data.extend_from_slice(&20u16.to_le_bytes()); // version
        self.data.extend_from_slice(&0u16.to_le_bytes()); // flags
        self.data.extend_from_slice(&method.to_le_bytes());
        self.data.extend_from_slice(&0u16.to_le_bytes()); // time
        self.data.extend_from_slice(&0u16.to_le_bytes()); // date
        self.data.extend_from_slice(&crc.to_le_bytes());
        self.data.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        self.data.extend_from_slice(&uncompressed_size.to_le_bytes());
        self.data.extend_from_slice(&(name.len() as u16).to_le_bytes());
        self.data.extend_from_slice(&0u16.to_le_bytes()); // extra
        self.data.extend_from_slice(name.as_bytes());
        self.data.extend_from_slice(raw);
        self.entries.push((
            name.to_string(),
            crc,
            raw.len() as u32,
            uncompressed_size,
            local_offset,
            method,
        ));
    }

    /// 加入一个不压缩的条目
    fn stored(&mut self, name: &str, content: &[u8]) {
        self.raw_entry(name, 0, content, content.len() as u32, crc32(content));
    }

    /// 加入一个 deflate 压缩的条目（真实 jar 里的常见形式）
    fn deflated(&mut self, name: &str, content: &[u8]) {
        let raw = deflate(content);
        self.raw_entry(name, 8, &raw, content.len() as u32, crc32(content));
    }

    fn finish(mut self) -> Vec<u8> {
        let cd_off = self.data.len() as u32;
        for (name, crc, comp, uncomp, off, method) in &self.entries {
            self.data.extend_from_slice(&CEN_SIG.to_le_bytes());
            self.data.extend_from_slice(&20u16.to_le_bytes()); // made by
            self.data.extend_from_slice(&20u16.to_le_bytes()); // needed
            self.data.extend_from_slice(&0u16.to_le_bytes()); // flags
            self.data.extend_from_slice(&method.to_le_bytes());
            self.data.extend_from_slice(&0u16.to_le_bytes());
            self.data.extend_from_slice(&0u16.to_le_bytes());
            self.data.extend_from_slice(&crc.to_le_bytes());
            self.data.extend_from_slice(&comp.to_le_bytes());
            self.data.extend_from_slice(&uncomp.to_le_bytes());
            self.data.extend_from_slice(&(name.len() as u16).to_le_bytes());
            self.data.extend_from_slice(&0u16.to_le_bytes()); // extra
            self.data.extend_from_slice(&0u16.to_le_bytes()); // comment
            self.data.extend_from_slice(&0u16.to_le_bytes()); // disk
            self.data.extend_from_slice(&0u16.to_le_bytes()); // internal attr
            self.data.extend_from_slice(&0u32.to_le_bytes()); // external attr
            self.data.extend_from_slice(&(*off as u32).to_le_bytes());
            self.data.extend_from_slice(name.as_bytes());
        }
        let cd_size = self.data.len() as u32 - cd_off;
        let total = self.entries.len() as u16;
        self.data.extend_from_slice(&EOCD_SIG.to_le_bytes());
        self.data.extend_from_slice(&0u16.to_le_bytes()); // disk
        self.data.extend_from_slice(&0u16.to_le_bytes()); // cd disk
        self.data.extend_from_slice(&total.to_le_bytes());
        self.data.extend_from_slice(&total.to_le_bytes());
        self.data.extend_from_slice(&cd_size.to_le_bytes());
        self.data.extend_from_slice(&cd_off.to_le_bytes());
        self.data.extend_from_slice(&0u16.to_le_bytes()); // comment len
        self.data
    }
}

/// 用 raw deflate 压缩（与 zip 条目的形式一致）
fn deflate(src: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(src).unwrap();
    e.finish().unwrap()
}

/// Deflate 条目的读取路径必须真的被覆盖。
///
/// 此前所有测试用的都是 stored（method 0）条目，deflate 分支
/// （format/zip.rs 的 Method::Deflate 解压）从未被执行过——而真实 jar 里的
/// 条目几乎全是 deflate，这是导入资源包时最主要的一条路径。
#[test]
fn reads_deflate_compressed_entry() {
    let mut b = ZipBuilder::new();
    // 内容取长一些并含重复片段，确保压缩后与原文长度不同，
    // 从而能区分「解压成功」与「原样返回压缩字节」
    let content = b"minecraft:block/stone\n".repeat(200);
    b.deflated("assets/stone.json", &content);
    let zip = b.finish();

    let a = open(&zip).expect("解析失败");
    let e = a.find("assets/stone.json").expect("应找到该条目");
    assert_eq!(e.method, Method::Deflate, "该条目应是 deflate 方式");
    assert_eq!(e.uncompressed_size as usize, content.len());

    let got = read(&zip, e).expect("应能解压读取");
    assert_eq!(got, content, "解压结果必须与原文一致");
}

/// 解压必须在「声明的量」处停下，而不是读完全部再比对长度。
///
/// 声明长度与解压展开比都取自文件，是不可信输入。原先 `.min(64 MiB)`
/// 只限制预分配容量、不限制增长，长度校验又排在 `read_to_end` 之后，
/// 于是一个「声明很小、实际能展开极大」的条目会先把内存吃光。
///
/// 这条直接调用 `inflate_limited` 并断言读到的字节数——内存边界因此
/// 可以被观测。仅断言「报错」是不够的：先解压后比对同样会报错。
///
/// 输入用手工构造的固定 Huffman deflate 流，而不是用配套的编码器：
/// 离线验证脚本里的 flate2 是桩，其**编码端只产出 stored 块**（不压缩），
/// 用它生成的流长度与原文相同，「解压后远大于输入」这个前提就不成立。
/// 手工构造的流在任何实现下都解出同一段内容（此处为 1 MB 全零）。
#[test]
fn inflate_limited_stops_at_declared_size() {
    let total = 1024 * 1024;
    let raw = zero_run_stream(total);
    // 前提校验：必须真的远小于解压结果，否则这条测试测不到展开攻击
    assert!(
        raw.len() < 64 * 1024,
        "该流应远小于解压结果，实际 {} 字节",
        raw.len()
    );

    let declared = 1024usize;
    let got = inflate_limited(&raw, declared).expect("解压应成功");
    assert_eq!(
        got.len(),
        declared + 1,
        "应恰好读到声明长度 + 1 字节就停下，而不是读完 {total} 字节"
    );

    // 对照：不限流时会读出完整内容，说明上面的上限确实起了作用
    use std::io::Read;
    let mut full = Vec::new();
    flate2::read::DeflateDecoder::new(&raw[..])
        .read_to_end(&mut full)
        .unwrap();
    assert_eq!(full.len(), total, "无限流时应解出完整 {total} 字节");
    assert!(
        full.len() > declared + 1,
        "两者差异必须显著，否则测试无意义"
    );
    assert!(full.iter().all(|&b| b == 0), "解出的内容应全是 0");
}

/// 构造一段解出 `total` 个 0x00 的固定 Huffman deflate 流。
///
/// 结构：块头（BFINAL=1、BTYPE=01 固定 Huffman）→ 一个字面量 0
/// → 反复「长度 258、距离 1」的匹配 → 块结束码。
/// 距离 1 的匹配会自我重叠复制，因此 1 MB 的零只需约 7 KB 的输入。
fn zero_run_stream(total: usize) -> Vec<u8> {
    /// deflate 的位打包：Huffman 码按 MSB 先写，头部字段按 LSB 先写
    struct Bits {
        out: Vec<u8>,
        cur: u8,
        n: u8,
    }
    impl Bits {
        fn bit(&mut self, b: u8) {
            self.cur |= (b & 1) << self.n;
            self.n += 1;
            if self.n == 8 {
                self.out.push(self.cur);
                self.cur = 0;
                self.n = 0;
            }
        }
        /// Huffman 码：高位先写
        fn code(&mut self, value: u32, len: u8) {
            for i in (0..len).rev() {
                self.bit(((value >> i) & 1) as u8);
            }
        }
        /// 头部字段：低位先写
        fn field(&mut self, value: u32, len: u8) {
            for i in 0..len {
                self.bit(((value >> i) & 1) as u8);
            }
        }
        fn finish(mut self) -> Vec<u8> {
            if self.n > 0 {
                self.out.push(self.cur);
            }
            self.out
        }
    }

    let mut b = Bits { out: Vec::new(), cur: 0, n: 0 };
    // 块头：BFINAL=1；BTYPE=01（固定 Huffman），按 LSB 先写
    b.field(1, 1);
    b.field(1, 2);
    // 固定 Huffman 中字面量 0 的码是 0x30（8 位）
    b.code(0x30, 8);
    let mut produced = 1usize;
    // 长度码 285 = 258 字节；距离码 0 = 距离 1
    while produced + 258 <= total {
        b.code(0xC5, 8); // 长度码 285
        b.code(0x00, 5); // 距离码 0
        produced += 258;
    }
    // 剩余不足 258 的用字面量补齐
    while produced < total {
        b.code(0x30, 8);
        produced += 1;
    }
    // 块结束码 256 = 0x00（7 位）
    b.code(0x00, 7);
    b.finish()
}

/// 声明长度恰好等于实际内容时，应完整拿到内容。
#[test]
fn inflate_limited_returns_exact_content_when_declared_matches() {
    let content = b"hello deflate world".repeat(100);
    let raw = deflate(&content);
    let got = inflate_limited(&raw, content.len()).expect("解压应成功");
    assert_eq!(got.len(), content.len(), "声明正确时应拿到完整内容");
    assert_eq!(got, content);
}

/// 声明长度远大于实际内容时，应只拿到实际那么多（由调用方报长度不符）。
#[test]
fn inflate_limited_returns_actual_when_declared_is_larger() {
    let content = b"short";
    let raw = deflate(content);
    let got = inflate_limited(&raw, 4096).expect("解压应成功");
    assert_eq!(got, content, "不应填充到声明长度，只返回真实解出的内容");
}

/// 声明长度与解压结果不符时必须报错。
///
/// 与上一条的区别：上一条验证「只读到声明的量」（内存边界），
/// 这一条验证「不符时报错」（正确性）。两者都要有。
#[test]
fn deflate_declared_size_mismatch_is_rejected() {
    let content = vec![b'x'; 2 * 1024 * 1024];
    let raw = deflate(&content);
    let mut b = ZipBuilder::new();
    b.raw_entry("bomb.bin", 8, &raw, 1024, crc32(&content));
    let zip = b.finish();

    let a = open(&zip).expect("解析失败");
    let e = a.find("bomb.bin").expect("应找到该条目");
    assert_eq!(e.uncompressed_size, 1024, "声明值应保持为 1 KB");

    let err = read(&zip, e).expect_err("超出声明长度应报错");
    assert!(
        err.to_string().contains("不符"),
        "错误信息应说明长度不符，实际：{err}"
    );
}

/// 存储方式也要校验长度：声明与实长不一致时不能放过。
#[test]
fn stored_length_mismatch_is_rejected() {
    let content = b"hello stored world";
    let mut b = ZipBuilder::new();
    // method 0（stored），但声明长度比实际多 3
    b.raw_entry("bad.txt", 0, content, content.len() as u32 + 3, crc32(content));
    let zip = b.finish();

    let a = open(&zip).expect("解析失败");
    let e = a.find("bad.txt").expect("应找到该条目");
    let err = read(&zip, e).expect_err("存储长度不符应报错");
    assert!(
        err.to_string().contains("不符"),
        "错误信息应说明长度不符，实际：{err}"
    );
}

/// CRC 不符时必须报错：数据在传输或存储中损坏了。
#[test]
fn deflate_crc_mismatch_is_rejected() {
    let content = b"hello deflate world";
    let raw = deflate(content);
    let mut b = ZipBuilder::new();
    b.raw_entry("bad.txt", 8, &raw, content.len() as u32, crc32(content) ^ 0xFF);
    let zip = b.finish();

    let a = open(&zip).expect("解析失败");
    // 先确认条目本身能被找到，避免把「找不到」误当成「CRC 校验生效」
    assert!(a.find("bad.txt").is_some());
    let e = a.find("bad.txt").expect("应找到该条目");
    let err = read(&zip, e).expect_err("CRC 不符应报错");
    assert!(
        err.to_string().contains("CRC") || err.to_string().contains("校验"),
        "错误信息应说明校验失败，实际：{err}"
    );
}

#[test]
fn lists_entries_from_central_directory() {
    let mut b = ZipBuilder::new();
    b.stored("assets/foo.txt", b"hello");
    b.stored("pack.mcmeta", b"{}");
    let zip = b.finish();

    let a = open(&zip).expect("解析失败");
    assert_eq!(a.entries.len(), 2);
    assert_eq!(a.names(), vec!["assets/foo.txt", "pack.mcmeta"]);
    assert_eq!(a.find("pack.mcmeta").unwrap().method, Method::Stored);
}

#[test]
fn stored_entry_reads_back_and_verifies_crc() {
    let mut b = ZipBuilder::new();
    b.stored("a.txt", b"content here");
    let zip = b.finish();
    let a = open(&zip).unwrap();
    let e = a.find("a.txt").unwrap();
    assert_eq!(read(&zip, e).unwrap(), b"content here");
}

#[test]
fn directory_entries_are_recognized() {
    let mut b = ZipBuilder::new();
    b.stored("assets/", b"");
    b.stored("assets/a.txt", b"x");
    let zip = b.finish();
    let a = open(&zip).unwrap();
    assert!(a.find("assets/").unwrap().is_dir());
    assert!(!a.find("assets/a.txt").unwrap().is_dir());
    assert_eq!(a.list_prefix("assets/").len(), 2);
}

#[test]
fn find_returns_none_for_missing_entry() {
    let mut b = ZipBuilder::new();
    b.stored("a.txt", b"x");
    let zip = b.finish();
    let a = open(&zip).unwrap();
    assert!(a.find("nope.txt").is_none());
    assert!(a.list_prefix("nope/").is_empty());
}

#[test]
fn rejects_non_zip_data() {
    let e = open(b"this is definitely not a zip file").unwrap_err().to_string();
    assert!(e.contains("中央目录"), "应指出找不到中央目录：{e}");
}

#[test]
fn rejects_truncated_file() {
    let e = open(b"PK").unwrap_err().to_string();
    assert!(e.contains("过短"), "应指出文件过短：{e}");
}

#[test]
fn eocd_is_found_despite_trailing_comment() {
    let mut b = ZipBuilder::new();
    b.stored("a.txt", b"data");
    let mut zip = b.finish();
    // 手工追加大段注释：EOCD 的注释长度字段保持 0 也能被找到，
    // 但真正的 zip 会写该字段；这里验证的是向后搜索的鲁棒性
    zip.extend_from_slice(&[0x41u8; 300]);
    let a = open(&zip).expect("应在尾部搜索中找到 EOCD");
    assert_eq!(a.entries.len(), 1);
}

/// 注释里出现假的 EOCD 签名时，必须仍然选中真正的那个。
///
/// zip 的注释字节是任意的（规范允许任何内容），因此注释里完全可能出现
/// `PK\x05\x06`。若从尾部向前只认「遇到的第一个签名」，遇到这种文件就会把
/// 假记录当成中央目录结尾：总条数、目录大小、目录偏移全取自假记录，
/// 结果是**打开成功但一条条目都找不到**，比直接报错更难排查。
///
/// 真正的 EOCD 满足「起点 + 22 + 注释长度 == 文件长度」，假的那条不满足
/// （它后面还跟着注释的其余字节），因此能用这个条件区分开。
///
/// 注意假记录必须**不在文件最末尾**：若它就压在最后 22 字节上，
/// 那条记录同样自洽，此时文件本身就是歧义的，任何实现都无法判定
/// （legacy-cpp 从尾部回扫，同样会先遇到它）。这里构造的是可判定的情形。
#[test]
fn fake_eocd_inside_comment_is_not_chosen() {
    let mut b = ZipBuilder::new();
    b.stored("a.txt", b"data");
    let mut zip = b.finish();

    // 注释 40 字节 = 假 EOCD（22 字节）+ 18 字节填充，
    // 使假记录落在 n-40 处，其后再有内容，因此它不自洽
    let mut comment: Vec<u8> = Vec::new();
    comment.extend_from_slice(&EOCD_SIG.to_le_bytes());
    comment.extend_from_slice(&0u16.to_le_bytes()); // disk
    comment.extend_from_slice(&0u16.to_le_bytes()); // cd disk
    comment.extend_from_slice(&0u16.to_le_bytes()); // 总条数 = 0（假）
    comment.extend_from_slice(&0u16.to_le_bytes()); // 总条数 = 0（假）
    comment.extend_from_slice(&0u32.to_le_bytes()); // cd_size = 0（假）
    comment.extend_from_slice(&0u32.to_le_bytes()); // cd_off  = 0（假）
    comment.extend_from_slice(&0u16.to_le_bytes()); // 注释长度 = 0（假）
    comment.extend_from_slice(&[0x41u8; 18]); // 填充：让假记录不再位于末尾
    assert_eq!(comment.len(), 40);

    // 真实 EOCD 的注释长度字段写成注释的实际长度，文件才自洽
    let eocd_at = zen(&zip);
    zip[eocd_at + 20..eocd_at + 22].copy_from_slice(&(comment.len() as u16).to_le_bytes());
    zip.extend_from_slice(&comment);

    let a = open(&zip).expect("应找到真正的中央目录结尾记录");
    assert_eq!(a.entries.len(), 1, "注释里的假记录总条数为 0，不能采信它");
    assert!(
        a.find("a.txt").is_some(),
        "条目必须可被找到；打开成功却找不到任何条目说明选错了 EOCD"
    );
}

/// 返回真实 EOCD 的偏移（测试辅助：从前往后找最后一条自洽的记录）
fn zen(data: &[u8]) -> usize {
    let mut i = 0;
    while i + 22 <= data.len() {
        if u32le(data, i) == EOCD_SIG {
            let clen = u16le(data, i + 20) as usize;
            if i + 22 + clen == data.len() {
                return i;
            }
        }
        i += 1;
    }
    panic!("测试构造的 zip 中没有自洽的 EOCD");
}

#[test]
fn crc_mismatch_is_reported() {
    let mut b = ZipBuilder::new();
    // 故意写入错误的 CRC
    b.raw_entry("bad.txt", 0, b"content", 7, 0xDEAD_BEEF);
    let zip = b.finish();
    let a = open(&zip).unwrap();
    let e = a.find("bad.txt").unwrap();
    let msg = read(&zip, e).unwrap_err().to_string();
    assert!(msg.contains("校验和"), "应指出校验和不符：{msg}");
}

#[test]
fn size_mismatch_is_reported() {
    let mut b = ZipBuilder::new();
    // 记录的解压后大小与实际不符
    b.raw_entry("bad.txt", 0, b"abc", 999, crc32(b"abc"));
    let zip = b.finish();
    let a = open(&zip).unwrap();
    let e = a.find("bad.txt").unwrap();
    let msg = read(&zip, e).unwrap_err().to_string();
    assert!(msg.contains("长度"), "应指出长度不符：{msg}");
}

#[test]
fn unsupported_compression_method_is_rejected() {
    let mut b = ZipBuilder::new();
    b.raw_entry("x.bin", 12, b"xxxx", 4, 0); // 12 = bzip2
    let zip = b.finish();
    let e = open(&zip).unwrap_err().to_string();
    assert!(e.contains("压缩方式"), "应指出压缩方式不支持：{e}");
}

#[test]
fn crc32_matches_known_vector() {
    // 标准测试向量：ASCII "123456789" 的 CRC-32 为 0xCBF43926
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b""), 0);
}

impl Archive {
    /// 按前缀列出条目名（Minecraft 资源常按目录组织）
    pub fn list_prefix<'a>(&'a self, prefix: &'a str) -> Vec<&'a Entry> {
        self.entries.iter().filter(|e| e.name.starts_with(prefix)).collect()
    }

    /// 全部条目名
    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.name.as_str()).collect()
    }
}
