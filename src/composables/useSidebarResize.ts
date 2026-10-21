/**
 * 左侧目录栏宽度的拖动调整。
 *
 * 拖动逻辑本身与方向无关，已收口到 usePanelResize；这里只给出目录栏这一侧
 * 的具体参数（横向、自己的合法区间、按窗口宽度算出的上限），并把成员名
 * 换成调用方习惯的 sidebar* 形式。
 *
 * 使用方式：`const { sidebarWidth, resizing, startResize, ... } = useSidebarResize()`，
 * 宽度值同时被界面状态记忆（store/uiState）读取与恢复。
 */
import { usePanelResize } from "./usePanelResize";

/** 名称列的默认宽度。取实测自然宽度（176px）的两倍。
 *
 * 名称是最需要宽度的列：投影文件名普遍较长，之前一律被省略号截断，
 * 只能靠悬停提示看全名。加倍后同屏能直接读出更多字符。
 *
 * 声明在这里而不是文件列表模块里：目录栏默认宽度要用它算出
 * 「能容下全部列」的宽度，文件列表的列定义也引用它。
 */
export const NAME_COL_W = 352;

/** 左侧目录栏宽度（像素）的合法区间。
 *
 * 太窄会让「名称/制作版本/尺寸」三列挤在一起、太宽则把右侧预览与详情
 * 压得过小。
 */
export const SIDEBAR_MIN_W = 320;
export const SIDEBAR_MAX_W = 1200;
/** 各列默认宽度之和，含选择列与 1px 占位列。改列宽时要同步。 */
const SIDEBAR_FIT_W = 40 + NAME_COL_W + 90 + 110 + 73 + 1;
/** 目录栏默认宽度。
 *
 * 取参考截图的实测值：截图宽 2047 设备像素、缩放 1.25（由已知的标题栏 32px
 * 与工具栏 44px 反推），表头里的竖直分隔线在设备像素 52/492/605/742/833，
 * 换算成 CSS 像素是 41.6 / 393.6 / 484 / 593.6 / 666.4，
 * 即各列 40（选择）/ 352 / 90 / 110 / 73，合计正好 666。
 *
 * 仍保留 SIDEBAR_FIT_W 兜底：万一以后把某列调宽到放不下，
 * 目录栏跟着变宽，而不是把末列挤出可视区。 */
const SIDEBAR_DEFAULT_W = Math.max(666, SIDEBAR_FIT_W);

export function useSidebarResize() {
  const panel = usePanelResize({
    axis: "x",
    defaultSize: SIDEBAR_DEFAULT_W,
    min: SIDEBAR_MIN_W,
    max: SIDEBAR_MAX_W,
    // 右侧详情面板至少要留出 360px，否则预览区会被挤没
    maxByWindow: () => window.innerWidth - 360,
  });

  return {
    sidebarWidth: panel.size,
    resizing: panel.resizing,
    clampSidebar: panel.clamp,
    startResize: panel.startResize,
    toggleSidebarWidth: panel.toggleSize,
    onSplitterKey: panel.onSplitterKey,
  };
}
