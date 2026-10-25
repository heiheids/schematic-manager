"""为命令层生成可离线编译的检查副本。

命令层依赖 tauri 与 serde 的过程宏，而过程宏无法用普通 Rust 源码替代，
因此这里把宏属性剥掉，只保留函数体与类型定义——**风险恰恰在函数体**
（跨模块接口调用、DTO 转换），那部分剥掉属性后仍会被完整检查。

被剥掉的内容：
  #[tauri::command]                          -> 删除
  #[serde(...)]                              -> 删除
  #[derive(... Serialize, Deserialize ...)]  -> 去掉这两个名字（含 serde:: 限定形式）
  #[cfg_attr(mobile, ...)]                   -> 删除

测试挂载声明保留，但 #[path] 改写到真实的 src/tests/commands/<name>.rs，
这样命令层的单元测试也能在离线检查里编译并运行。

用法：python gen_cmd_check.py
"""

import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "..", "..", "src-tauri", "src")
COMMANDS = os.path.join(SRC, "commands")
OUT = os.path.join(HERE, "generated", "commands")
TESTS_REL = "../../../../src-tauri/src/tests/commands"


def strip_macros(text: str) -> str:
    # #[tauri::command] 与 #[tauri::command(...)]
    text = re.sub(r"^[ \t]*#\[tauri::command[^\]]*\]\s*\n", "", text, flags=re.M)
    # #[serde(...)] 可能跨行
    text = re.sub(r"^[ \t]*#\[serde\([^\n]*?\)\]\s*\n", "", text, flags=re.M)
    # #[cfg_attr(mobile, tauri::mobile_entry_point)] 之类
    text = re.sub(r"^[ \t]*#\[cfg_attr\(mobile[^\n]*?\)\]\s*\n", "", text, flags=re.M)

    # derive 里去掉 Serialize / Deserialize（含 serde:: 限定写法）
    def fix_derive(m: "re.Match") -> str:
        inner = m.group(1)
        parts = [p.strip() for p in inner.split(",")]
        keep = [
            p
            for p in parts
            if p not in ("Serialize", "Deserialize")
            and not p.endswith("::Serialize")
            and not p.endswith("::Deserialize")
        ]
        if not keep:
            return ""
        return "#[derive(" + ", ".join(keep) + ")]"

    return re.sub(r"#\[derive\(([^)]*)\)\]", fix_derive, text)


def rewrite_test_path(text: str, name: str) -> str:
    """把测试挂载指向真实测试文件：生成副本位于 tools/typecheck/generated/commands/。"""
    target = TESTS_REL + "/" + name
    return re.sub(r'#\[path = "[^"]+"\]', '#[path = "' + target + '"]', text)


def main() -> None:
    os.makedirs(OUT, exist_ok=True)
    names = sorted(n for n in os.listdir(COMMANDS) if n.endswith(".rs"))
    for name in names:
        src = os.path.join(COMMANDS, name)
        with open(src, encoding="utf-8") as f:
            text = f.read()
        out = rewrite_test_path(strip_macros(text), name)
        removed = text.count("#[tauri::command]") + text.count("#[serde(")
        with open(os.path.join(OUT, name), "w", encoding="utf-8", newline="\n") as f:
            f.write(out)
        print("%-18s 剥离宏属性 %d 处 -> generated/commands/%s" % (name, removed, name))


if __name__ == "__main__":
    main()
