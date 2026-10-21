/**
 * 跟随系统深浅色主题。
 *
 * Arco 的深色变量挂在 `body[arco-theme="dark"]` 上，而样式表里两套取值都
 * 已经写好（见 styles/theme.scss 与自定义样式里的深色分支）——但没人设置
 * 这个属性的话，那些深色取值永远不会生效：系统开了深色，界面仍是白的。
 * 这里把窗口的实际主题写到 body 上，并在系统切换时跟随。
 *
 * 读不到主题时（例如在浏览器里跑前端、或平台不提供该能力）保持浅色，
 * 不影响其余功能。
 *
 * 只在应用根组件使用：主题是全局的，各页面/组件不应各自再跟随一遍。
 */
import { getCurrentWindow } from "@tauri-apps/api/window";
import { reportUiWarning } from "../utils/run_log_report";

export function useTheme() {
  async function followSystemTheme() {
    const appWindow = getCurrentWindow();
    try {
      const apply = (t: string | null) => {
        if (t === "dark") document.body.setAttribute("arco-theme", "dark");
        else document.body.removeAttribute("arco-theme");
      };
      apply(await appWindow.theme());
      await appWindow.onThemeChanged(({ payload }) => apply(payload));
    } catch (e) {
      // 不静默吞掉：这类失败会让界面主题与系统不一致，需要留下线索
      reportUiWarning("读取系统主题失败，按浅色显示：", e);
    }
  }

  return { followSystemTheme };
}
