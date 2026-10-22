/** 批量导出预览图时的输出文件名计算。
 *
 * 抽成独立模块的原因：这里是「导出 10 张最后只剩 1 张」这类错误的所在——
 * 不同目录下可能有同名投影，若直接按文件名写就互相覆盖，而且没有任何提示。
 * 逻辑独立后才能直接测，不必挂载整个组件。
 */

/** 一批导出里已占用的文件名 */
export class NamePool {
  private used = new Set<string>();

  /**
   * 取一个本批内尚未用过的文件名。
   *
   * 重名时按 `名字 (2).png`、`名字 (3).png` 递增，与后端复制文件时的
   * 序号规则一致，用户看到的命名风格保持统一。
   *
   * 只解决**同一批内**的重名，不检查目标目录里已有的文件：
   * 是否覆盖既有文件交给写入语义，用户在选择目录时已经知道那里有什么；
   * 此处若私自改名，反而让「为什么这次导出的文件名不一样」难以解释。
   */
  take(stem: string, ext: string): string {
    const first = `${stem}${ext}`;
    if (!this.used.has(first)) {
      this.used.add(first);
      return first;
    }
    for (let i = 2; i < 10_000; i++) {
      const cand = `${stem} (${i})${ext}`;
      if (!this.used.has(cand)) {
        this.used.add(cand);
        return cand;
      }
    }
    // 一万个同名文件的极端情况：给一个确定性的兜底名，
    // 而不是抛错中断整批导出
    const fallback = `${stem} (${this.used.size + 1})${ext}`;
    this.used.add(fallback);
    return fallback;
  }
}

/** 由投影文件名与实际像素宽高生成导出文件的「主干」部分（不含扩展名） */
export function previewStem(fileName: string, w: number, h: number): string {
  // 去掉 .litematic 后缀（大小写不敏感）：投影文件名是用户可见的，
  // 保留后缀会得到 `A.litematic_preview_1024x768.png`
  //
  // 用宽×高而不是短边单个数：矩形导出的两种比例可能有相同的短边，
  // 只写短边的话 1024×768 与 1024×1024 会得到同名文件。
  return `${fileName.replace(/\.litematic$/i, "")}_preview_${w}x${h}`;
}

/**
 * 拼一个输出文件路径。
 *
 * 刻意**不用** `@tauri-apps/api/path` 的 `join`：它内部走
 * `invoke("plugin:path|join")`，而本项目只注册了 dialog 与 fs 两个插件,
 * 没有 tauri-plugin-path（已核对 Cargo.lock 与 lib.rs），Tauri 2 自身
 * 也不提供 `plugin:path|*` 命令。调用它会在运行时以「命令不存在」失败，
 * 表现为批量导出一张图都写不出来。
 *
 * 改为按目标目录已有分隔符拼接：目录选择框返回的路径已经是平台形式
 * （Windows 上是 `C:\foo`），跟随它自己的分隔符即可，不混用。
 */
export function joinPath(dir: string, name: string): string {
  const sep = dir.includes("\\") && !dir.includes("/") ? "\\" : "/";
  return dir.endsWith(sep) ? `${dir}${name}` : `${dir}${sep}${name}`;
}
