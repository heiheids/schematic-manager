import { describe, expect, it } from "vitest";
import { joinPath, NamePool, previewStem } from "../render/export_name";

describe("输出路径拼接", () => {
  it("跟随目标目录自己的分隔符，不混用", () => {
    // Windows 路径：目录选择框返回的是反斜杠形式
    expect(joinPath("C:\\dir", "a.png")).toBe("C:\\dir\\a.png");
    // Unix / 多数 Tauri 返回的正斜杠形式
    expect(joinPath("/home/u/dir", "a.png")).toBe("/home/u/dir/a.png");
    // 驱动器根目录：不应漏掉分隔符
    expect(joinPath("C:\\", "a.png")).toBe("C:\\a.png");
    expect(joinPath("/", "a.png")).toBe("/a.png");
  });

  it("目录已带结尾分隔符时不重复添加", () => {
    expect(joinPath("C:\\dir\\", "a.png")).toBe("C:\\dir\\a.png");
    expect(joinPath("/home/u/dir/", "a.png")).toBe("/home/u/dir/a.png");
  });

  it("路径含中文与空格也正确拼接", () => {
    expect(joinPath("D:\\我的 目录", "投影_preview_512.png")).toBe(
      "D:\\我的 目录\\投影_preview_512.png",
    );
  });
});

describe("导出文件名", () => {
  it("去掉 .litematic 后缀并带上宽高", () => {
    expect(previewStem("A.litematic", 1024, 1024)).toBe("A_preview_1024x1024");
    // 后缀大小写不敏感：真实文件里有 .LITEMATIC
    expect(previewStem("B.LITEMATIC", 1024, 768)).toBe("B_preview_1024x768");
    // 不含后缀的文件名不应被截断
    expect(previewStem("无后缀", 768, 1024)).toBe("无后缀_preview_768x1024");
    // 名字里本来就有点，只去末尾的 .litematic
    expect(previewStem("v1.2.litematic", 1366, 768)).toBe("v1.2_preview_1366x768");
  });

  it("同样的短边、不同的比例得到不同文件名，不会互相覆盖", () => {
    // 这是改用「宽×高」而不是只写短边的原因：1024 是 1:1 与 4:3 共有的短边
    expect(previewStem("A.litematic", 1024, 1024)).not.toBe(
      previewStem("A.litematic", 1024, 768),
    );
  });

  it("同一批内重名时自动加序号，互不覆盖", () => {
    // 这正是「导出 10 张最后只剩 1 张」的防线：不同目录下可以有同名投影
    const pool = new NamePool();
    const names = [
      pool.take("A_preview_512", ".png"),
      pool.take("A_preview_512", ".png"),
      pool.take("A_preview_512", ".png"),
    ];
    expect(names).toEqual([
      "A_preview_512.png",
      "A_preview_512 (2).png",
      "A_preview_512 (3).png",
    ]);
    expect(new Set(names).size, "三个名字必须互不相同").toBe(3);
  });

  it("不同名字互不影响", () => {
    const pool = new NamePool();
    expect(pool.take("A_preview_512", ".png")).toBe("A_preview_512.png");
    expect(pool.take("B_preview_512", ".png")).toBe("B_preview_512.png");
    expect(pool.take("A_preview_512", ".png")).toBe("A_preview_512 (2).png");
    expect(pool.take("B_preview_512", ".png")).toBe("B_preview_512 (2).png");
  });

  it("大量同名也能给出唯一名字", () => {
    const pool = new NamePool();
    const seen = new Set<string>();
    for (let i = 0; i < 300; i++) {
      const n = pool.take("same", ".png");
      expect(seen.has(n), `第 ${i} 个名字 ${n} 重复了`).toBe(false);
      seen.add(n);
    }
    expect(seen.size).toBe(300);
  });

  it("序号跳过的位置会被复用（前一个名字未被占用时）", () => {
    // 先占 (2)，再占原始名，此时应给出 (3) 而不是重复 (2)
    const pool = new NamePool();
    expect(pool.take("X", ".png")).toBe("X.png");
    expect(pool.take("X", ".png")).toBe("X (2).png");
    expect(pool.take("X", ".png")).toBe("X (3).png");
  });
});
