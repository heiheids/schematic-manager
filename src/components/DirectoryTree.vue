<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useAppStore } from "../store";
import DirectoryTreeNode from "./DirectoryTreeNode.vue";

const { t } = useI18n();
const { roots, loadedDirs } = useAppStore();
</script>

<template>
  <aside class="directory-tree">
    <div class="directory-tree__title">{{ t("fileManager.directoryTree") }}</div>
    <a-scrollbar type="embed" class="scroll-fill__inner" outer-class="scroll-fill">
      <ul v-if="roots.length" class="directory-tree__roots">
        <DirectoryTreeNode
          v-for="item in roots"
          :key="item.key"
          :root="item.root"
          :node="item.node"
        />
      </ul>
      <a-empty v-else :description="t(loadedDirs.length ? 'fileManager.noDirectoriesEnabled' : 'fileManager.emptyRoot')" />
    </a-scrollbar>
  </aside>
</template>
