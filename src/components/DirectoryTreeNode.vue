<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { useAppStore } from "../store";
import { directoryKey } from "../store/fileManager";
import type { DirectoryNode } from "../types";

defineOptions({ name: "DirectoryTreeNode" });

const props = withDefaults(
  defineProps<{
    root: string;
    node: DirectoryNode;
    level?: number;
  }>(),
  { level: 0 },
);

const { t } = useI18n();
const { selectedDirKey, selectDirectory, toggleDirectory, isExpanded } = useAppStore();
const key = computed(() => directoryKey(props.root, props.node));
const expanded = computed(() => isExpanded(key.value));
const hasChildren = computed(() => props.node.children.length > 0);
</script>

<template>
  <li class="directory-node">
    <div
      class="directory-node__row"
      :class="{ 'directory-node__row--active': selectedDirKey === key }"
      :style="{ paddingLeft: `${level * 14 + 4}px` }"
    >
      <button
        v-if="hasChildren"
        class="directory-node__expander"
        :aria-label="expanded ? t('files.collapseDirectory') : t('files.expandDirectory')"
        @click.stop="toggleDirectory(key)"
      >
        {{ expanded ? "▾" : "▸" }}
      </button>
      <span v-else class="directory-node__expander-space" />
      <button class="directory-node__select" @click="selectDirectory(key)">
        <span
          class="directory-icon"
          :class="{ 'directory-icon--solid': node.filePaths.length > 0 }"
          aria-hidden="true"
        />
        <span class="directory-node__name" :title="node.path">{{ node.name }}</span>
        <span v-if="node.filePaths.length" class="directory-node__count">
          {{ node.filePaths.length }}
        </span>
        <span v-if="node.truncated" class="directory-node__truncated" :title="t('fileManager.truncated')">
          !
        </span>
      </button>
    </div>
    <ul v-if="hasChildren && expanded" class="directory-node__children">
      <DirectoryTreeNode
        v-for="child in node.children"
        :key="directoryKey(root, child)"
        :root="root"
        :node="child"
        :level="level + 1"
      />
    </ul>
  </li>
</template>
