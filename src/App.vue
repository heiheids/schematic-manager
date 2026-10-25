<script setup lang="ts">
/**
 * 应用根组件：只做装配。
 *
 * 业务状态与逻辑全部在 store/ 的各域里，工作区与各弹窗各自成组件。
 * 这里负责创建应用状态、跟随系统主题，并按活动选项卡装配当前面板。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import AppToolbar from "./components/AppToolbar.vue";
import BackupDrawer from "./components/BackupDrawer.vue";
import ConvertModal from "./components/ConvertModal.vue";
import DetailPanel from "./components/DetailPanel.vue";
import FileManagerPanel from "./components/FileManagerPanel.vue";
import LogModal from "./components/LogModal.vue";
import ReplaceModal from "./components/ReplaceModal.vue";
import SettingsPanel from "./components/SettingsPanel.vue";
import SetupModal from "./components/SetupModal.vue";
import TitleBar from "./components/TitleBar.vue";
import UpdateModal from "./components/UpdateModal.vue";
import WorkspaceTabs from "./components/WorkspaceTabs.vue";
import { useTheme } from "./composables/useTheme";
import { arcoLocaleOf, currentLocale } from "./i18n";
import { provideAppStore } from "./store";

// 主题跟随在挂载前启动：它异步读取并订阅系统主题，与状态装配无关
const { followSystemTheme } = useTheme();

// 语言切换要连 Arco 自己的内置文案（分页、空数据、弹窗按钮）一起换
const arcoLocale = computed(() => arcoLocaleOf(currentLocale()));

const { t } = useI18n();

const s = provideAppStore();

const { activeTab, selected, statusText, openFiles, openSettings } = s;

// 启动即跟随系统主题；不等它完成，免得拖慢首屏
void followSystemTheme();
</script>

<template>
  <a-config-provider :locale="arcoLocale">
    <div class="app-shell">
      <TitleBar />

      <WorkspaceTabs />

      <AppToolbar v-if="activeTab?.kind === 'files'" />
      <FileManagerPanel v-if="activeTab?.kind === 'files'" />
      <div v-else-if="activeTab?.kind === 'editor'" class="workbench">
        <DetailPanel />
      </div>
      <SettingsPanel v-else-if="activeTab?.kind === 'settings'" />
      <div v-else class="workspace-empty">
        <a-empty :description="t('workspace.empty')" />
        <a-space>
          <a-button size="small" @click="openFiles">{{ t("workspace.openFiles") }}</a-button>
          <a-button size="small" @click="openSettings">{{ t("workspace.openSettings") }}</a-button>
        </a-space>
      </div>

      <ConvertModal />

      <SetupModal />

      <LogModal />

      <ReplaceModal />

      <UpdateModal />

      <BackupDrawer />

      <div class="statusbar">
        <span>{{ statusText }}</span>
        <span class="statusbar__spacer" />
        <span v-if="selected">{{ t("app.selectedFile", { name: selected.fileName }) }}</span>
      </div>
    </div>
  </a-config-provider>
</template>
