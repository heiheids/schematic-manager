/**
 * 自定义标题栏：应用标识 + 界面语言切换 + 窗口控制按钮（最小化/最大化/关闭）。
 *
 * 窗口外壳行为，与业务状态无关，因此直接在这里调 Tauri 窗口 API，
 * 不进 store。拖动区由 data-tauri-drag-region 标记，交给系统处理。
 *
 * 语言切换放在这里而不是工具条：工具条有十来个按钮，实测约需 1650px，
 * 窗口最小宽度却是 1400，放最右侧会被挤出可视范围；标题栏右侧始终有空位。
 *
 * 左上角的标识直接取图标文件本身（`src/assets/app-icon.png`，由
 * `tools/gen_app_icons.py` 从 `docs/48x.png` 生成），不再手写一份 SVG：
 * 手写的那份曾经因为图标换版而没跟着改，界面上就一直显示旧标识。
 * 从母版出图，标识与程序图标就不可能再脱节。
 */
<script setup lang="ts">
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useI18n } from "vue-i18n";
import appIcon from "../assets/app-icon.png";
import { LOCALES, currentLocale, setLocale } from "../i18n";
import { useAppStore } from "../store";

const { t } = useI18n();

const { confirmDiscard } = useAppStore();

const appWindow = getCurrentWindow();
const minimize = () => appWindow.minimize();
const toggleMaximize = () => appWindow.toggleMaximize();

/** 关窗前先问一句：有未保存的改动时直接关掉，用户会以为已经存过了。
 *
 * 应用的关窗入口就是这一个按钮（标题栏自绘，decorations 为 false），
 * 因此拦在这里即可覆盖「关窗口」与「退出应用」两种情形——它们走的是同一个动作。
 */
async function close() {
  if (!(await confirmDiscard("files.unsavedClose"))) return;
  await appWindow.close();
}

/** 当前语言的按钮文字：认不出来的语言取清单里第一项，与 i18n 的回落一致 */
function currentLabelKey(): string {
  return LOCALES.find((item) => item.locale === currentLocale())!.labelKey;
}
</script>

<template>
  <div class="titlebar" data-tauri-drag-region>
    <div class="titlebar__logo" aria-hidden="true">
      <img :src="appIcon" alt="" width="18" height="18" />
    </div>
    <div class="titlebar__title">{{ t("titleBar.appName") }}</div>
    <div class="titlebar__spacer" data-tauri-drag-region />
    <!-- 语言切换：界面右上角。按钮不带 data-tauri-drag-region，
         因此点它不会变成拖动窗口 -->
    <a-dropdown class="titlebar__lang" trigger="click" position="br">
      <a-button size="mini" type="text" :title="t('lang.label')">
        <icon-language />
        {{ t(currentLabelKey()) }}
      </a-button>
      <template #content>
        <a-doption
          v-for="item in LOCALES"
          :key="item.locale"
          :value="item.locale"
          @click="setLocale(item.locale)"
        >
          {{ t(item.labelKey) }}
        </a-doption>
      </template>
    </a-dropdown>
    <div class="titlebar__btn" @click="minimize">&#x2500;</div>
    <div class="titlebar__btn" @click="toggleMaximize">&#x25A1;</div>
    <div class="titlebar__btn is-close" @click="close">&#x2715;</div>
  </div>
</template>
