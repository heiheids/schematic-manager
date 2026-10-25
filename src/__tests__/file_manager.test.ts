import { describe, expect, it } from "vitest";
import { collectDirectoryFiles, directoryKey } from "../store/fileManager";
import type { DirectoryNode } from "../types";

function node(
  relativePath: string,
  filePaths: string[] = [],
  children: DirectoryNode[] = [],
  truncated = false,
): DirectoryNode {
  const name = relativePath.split(/[\\/]/).filter(Boolean).pop() ?? relativePath;
  return {
    path: relativePath ? `/root/${relativePath}` : "/root",
    name: name || "root",
    relativePath,
    filePaths,
    children,
    truncated,
  };
}

describe("文件管理目录树", () => {
  it("关闭子目录开关只取直接文件，开启后取全部后代文件", () => {
    const tree = node("", ["/root/a.litematic"], [
      node("empty"),
      node("child", ["/root/child/b.litematic"], [
        node("nested", ["/root/child/nested/c.litematic"]),
      ]),
    ]);
    expect([...collectDirectoryFiles(tree, false)]).toEqual(["/root/a.litematic"]);
    expect([...collectDirectoryFiles(tree, true)].sort()).toEqual([
      "/root/a.litematic",
      "/root/child/b.litematic",
      "/root/child/nested/c.litematic",
    ]);
  });

  it("空目录保留在树中，深度截止节点单独标记", () => {
    const empty = node("empty", [], [], false);
    const truncated = node("deep", [], [], true);
    expect(empty.children).toEqual([]);
    expect(empty.truncated).toBe(false);
    expect(truncated.truncated).toBe(true);
    expect(collectDirectoryFiles(empty, true).size).toBe(0);
  });

  it("相同相对路径在不同根目录下不会共用节点 key", () => {
    const child = node("child");
    expect(directoryKey("/a", child)).not.toBe(directoryKey("/b", child));
    expect(directoryKey("/a", child)).toBe(directoryKey("/a", child));
  });
});
