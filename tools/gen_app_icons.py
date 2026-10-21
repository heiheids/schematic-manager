"""从两张母版生成应用图标集，以及界面里当标识用的那几张位图。

为什么需要这个脚本：图标按显示尺寸分两档——小于等于 48 像素的用
`docs/48x.png`（去掉了取景框那类细装饰、主图更大，缩到十几像素时才不糊），
大于 48 的用 `docs/256x.png`。而 `pnpm tauri icon` 只接受一张母版，没法按尺寸
混用两张，所以 `.ico` 与 `.icns` 这两个「一个文件装多档尺寸」的容器得自己组装。

界面标题栏的标识也在这里出图（`src/assets/`），而不是让组件手写一份 SVG：
手写的那份曾经因为图标换版而没跟着改，界面上就一直显示旧标识——
从母版出图，标识与程序图标就不可能再脱节。

组装靠 Pillow 的 `append_images`：对每个目标尺寸给一张尺寸**完全相等**的图，
Pillow 就直接采用它，而不是把基准图缩下来（见 IcoImagePlugin / IcnsImagePlugin
的 `_save`）。因此只要按档位挑对母版渲染，容器里就是两张母版混着的。

用法：
    python tools/gen_app_icons.py

产出覆盖 `src-tauri/icons/`。注意：只改图标文件时增量构建不会重跑 build.rs，
exe 会继续留着旧图标资源，验证前要清掉本 crate 的 build-script 缓存。
"""
import io
import os
import struct
import sys

from PIL import Image, ImageChops

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICON_DIR = os.path.join(ROOT, "src-tauri", "icons")

# 两张母版：名字里的 256/48 指的是它们各自负责的尺寸带上限
MASTER_LARGE = os.path.join(ROOT, "docs", "256x.png")
MASTER_SMALL = os.path.join(ROOT, "docs", "48x.png")
SMALL_MAX = 48

# 落盘的 PNG 档
PNG_OUTPUTS = (
    (32, "32x32.png"),
    (64, "64x64.png"),
    (128, "128x128.png"),
    (256, "128x128@2x.png"),
    (512, "icon.png"),
)

# 界面标识：标题栏按 18px 显示，出一张 3 倍的图由浏览器缩到目标尺寸，
# 这样 1x/2x/3x 屏都清晰。固定用小母版——它去掉了细装饰，缩到十几像素才不糊。
UI_LOGO_DIR = os.path.join(ROOT, "src", "assets")
UI_LOGO_SIZE = 54
UI_LOGO_NAME = "app-icon.png"

# 容器内的档位。ICO 的集合与替换前一致（Pillow 默认还含 128，必须显式覆盖）
ICO_SIZES = (16, 24, 32, 48, 64, 256)
ICNS_TYPES = (
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic13", 256),
    (b"ic09", 512),
    (b"ic14", 512),
    (b"ic10", 1024),
)


def load_masters():
    """读两张母版，确认是正方形且带透明通道。"""
    out = {}
    for label, path in (("large", MASTER_LARGE), ("small", MASTER_SMALL)):
        if not os.path.exists(path):
            sys.exit(f"缺少母版：{path}")
        img = Image.open(path).convert("RGBA")
        if img.width != img.height:
            sys.exit(f"母版必须是正方形：{path} 是 {img.width}x{img.height}")
        out[label] = img
    return out


def master_for(size):
    """该档尺寸该用哪张母版。阈值就是需求本身。"""
    return "small" if size <= SMALL_MAX else "large"


def render(masters, size):
    """按档位挑母版并渲染出该尺寸。"""
    return masters[master_for(size)].resize((size, size), Image.Resampling.LANCZOS)


def write_pngs(masters):
    for size, name in PNG_OUTPUTS:
        render(masters, size).save(os.path.join(ICON_DIR, name), "PNG")


def write_ui_logo(masters):
    """写界面标识用的位图：固定取小母版，与图标集的小尺寸档同源。"""
    os.makedirs(UI_LOGO_DIR, exist_ok=True)
    logo = masters["small"].resize((UI_LOGO_SIZE, UI_LOGO_SIZE), Image.Resampling.LANCZOS)
    logo.save(os.path.join(UI_LOGO_DIR, UI_LOGO_NAME), "PNG")


def write_ico(masters):
    """写多档 ICO：16/24/32/48 来自小母版，64/256 来自大母版。"""
    images = [render(masters, s) for s in ICO_SIZES]
    # 基准图只用来通过 Pillow 的尺寸过滤（它要求请求尺寸不大于基准图），
    # 每一档都由 append_images 里尺寸相等的那张提供，基准图本身不会被写入
    base = render(masters, 512)
    base.save(
        os.path.join(ICON_DIR, "icon.ico"),
        format="ICO",
        sizes=[(s, s) for s in ICO_SIZES],
        append_images=images,
    )


def write_icns(masters):
    """写 macOS 的 ICNS。Pillow 只会写 ic07~ic14 这八档 PNG。"""
    sizes = sorted({size for _, size in ICNS_TYPES})
    images = [render(masters, s) for s in sizes]
    base = render(masters, 512)
    base.save(os.path.join(ICON_DIR, "icon.icns"), format="ICNS", append_images=images)


def read_ico(path):
    with open(path, "rb") as f:
        data = f.read()
    _reserved, kind, count = struct.unpack_from("<HHH", data, 0)
    if kind != 1:
        raise ValueError(f"{path} 不是 ICO（type={kind}）")
    entries = []
    for i in range(count):
        w, _h, _c, _r, _p, _b, size, offset = struct.unpack_from("<BBBBHHII", data, 6 + 16 * i)
        entries.append((w or 256, data[offset:offset + size]))
    return entries


def read_icns(path):
    with open(path, "rb") as f:
        data = f.read()
    magic, _total = struct.unpack_from(">4si", data, 0)
    if magic != b"icns":
        raise ValueError(f"{path} 不是 ICNS")
    chunks = {}
    pos = 8
    while pos + 8 <= len(data):
        ctype = data[pos:pos + 4]
        length = struct.unpack_from(">i", data, pos + 4)[0]
        if length < 8:
            raise ValueError(f"{path} 的分块 {ctype!r} 长度非法：{length}")
        chunks.setdefault(ctype, []).append(data[pos + 8:pos + length])
        pos += length
    return chunks


def assert_same_pixels(actual_png, expected, label):
    actual = Image.open(io.BytesIO(actual_png)).convert("RGBA")
    if actual.size != expected.size:
        raise AssertionError(f"{label}：尺寸不符，文件里是 {actual.size}，应是 {expected.size}")
    if ImageChops.difference(actual, expected).getbbox() is not None:
        raise AssertionError(f"{label}：像素与应该使用的那张母版渲染结果不一致")


def verify(masters):
    """读回两个容器，逐档核对尺寸与来源。

    不读回就只是在信任 Pillow 的 append_images 语义（它按尺寸精确匹配，匹配不上
    时才回退缩基准图），而「哪一档用了哪张母版」正是本次最容易错的地方。
    """
    ico_path = os.path.join(ICON_DIR, "icon.ico")
    entries = read_ico(ico_path)
    got_sizes = [w for w, _ in entries]
    if got_sizes != sorted(ICO_SIZES):
        raise AssertionError(f"ICO 档位不符：{got_sizes}，应为 {sorted(ICO_SIZES)}")
    for (width, payload), size in zip(entries, sorted(ICO_SIZES)):
        assert_same_pixels(payload, render(masters, width), f"ICO {width}px")

    icns_path = os.path.join(ICON_DIR, "icon.icns")
    chunks = read_icns(icns_path)
    # Pillow 会额外写一个可选的 TOC 分块，其余分块必须正好是预期的那些
    extra = set(chunks) - {b"TOC "} - {t for t, _ in ICNS_TYPES}
    if extra:
        raise AssertionError(f"ICNS 出现意外分块：{sorted(extra)}")
    for ctype, size in ICNS_TYPES:
        if ctype not in chunks:
            raise AssertionError(f"ICNS 缺少分块 {ctype!r}")
        for payload in chunks[ctype]:
            assert_same_pixels(payload, render(masters, size), f"ICNS {ctype.decode()} ({size}px)")


def main():
    masters = load_masters()
    print(f"大母版（>48）：{os.path.relpath(MASTER_LARGE, ROOT)}  {masters['large'].size[0]}px")
    print(f"小母版（<=48）：{os.path.relpath(MASTER_SMALL, ROOT)}  {masters['small'].size[0]}px")

    os.makedirs(ICON_DIR, exist_ok=True)
    # 放大档要提前说清楚：母版只有 512，尺寸带里却有 1024
    for size in sorted({s for s, _ in PNG_OUTPUTS} | set(ICO_SIZES) | {s for _, s in ICNS_TYPES}):
        src = masters[master_for(size)]
        if size > src.width:
            which = "大母版" if master_for(size) == "large" else "小母版"
            print(f"  提醒：{size} 档由{which}（{src.width}px）放大而来，边缘会比原生略软")

    write_pngs(masters)
    write_ico(masters)
    write_icns(masters)
    write_ui_logo(masters)

    verify(masters)
    print(f"已写出并校验：{os.path.relpath(ICON_DIR, ROOT)}")
    for size, name in PNG_OUTPUTS:
        band = "小母版" if size <= SMALL_MAX else "大母版"
        print(f"  {name:<16} {size:>4}px  {band}")
    print(f"  {'icon.ico':<16} {sorted(ICO_SIZES)}")
    print(f"  {'icon.icns':<16} {sorted({s for _, s in ICNS_TYPES})}")
    print(
        f"界面标识：{os.path.relpath(os.path.join(UI_LOGO_DIR, UI_LOGO_NAME), ROOT)}"
        f"  {UI_LOGO_SIZE}px（小母版）"
    )


if __name__ == "__main__":
    main()
