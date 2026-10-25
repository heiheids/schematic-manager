<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useAppStore } from "../store";

const { t } = useI18n();
const { tabs, activeTabId, activateTab, closeTab, openFiles, openSettings } = useAppStore();

function titleOf(tab: (typeof tabs.value)[number]): string {
  if (tab.kind === "files") return t("workspace.files");
  if (tab.kind === "settings") return t("workspace.settings");
  return tab.title;
}
</script>

<template>
  <div class="workspace-tabs">
    <div class="workspace-tabs__list" role="tablist" :aria-label="t('workspace.editor')">
      <div
        v-for="tab in tabs"
        :key="tab.id"
        class="workspace-tab"
        :class="{ 'workspace-tab--active': activeTabId === tab.id }"
      >
        <button
          class="workspace-tab__label"
          role="tab"
          :aria-selected="activeTabId === tab.id"
          :title="tab.path || titleOf(tab)"
          @click="activateTab(tab.id)"
        >
          {{ titleOf(tab) }}
        </button>
        <button
          class="workspace-tab__close"
          :aria-label="t('workspace.closeTab')"
          :title="t('workspace.closeTab')"
          @click.stop="closeTab(tab.id)"
        >
          ×
        </button>
      </div>
    </div>
    <div class="workspace-tabs__actions">
      <a-button size="mini" type="text" :title="t('workspace.openFiles')" @click="openFiles">
        {{ t("workspace.files") }}
      </a-button>
      <a-button size="mini" type="text" :title="t('workspace.openSettings')" @click="openSettings">
        {{ t("workspace.settings") }}
      </a-button>
    </div>
  </div>
</template>
