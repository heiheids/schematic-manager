import type { DirectoryNode, SchematicSummary } from "../types";

/** 为旧扫描结果或测试替身补一个直接文件根节点，完整目录树不由这里重建。 */
export function directoryRootOrDefault(
  dir: string,
  root: DirectoryNode | undefined,
  items: SchematicSummary[],
): DirectoryNode {
  if (root) return root;
  const name = dir.split(/[\\/]/).filter(Boolean).pop() ?? dir;
  return {
    path: dir,
    name,
    relativePath: "",
    filePaths: items.map((item) => item.path),
    children: [],
    truncated: false,
  };
}
