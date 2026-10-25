<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useAppStore } from "../store";
import { useListKeyboard } from "../composables/useListKeyboard";
import SchematicCard from "./SchematicCard.vue";

const { t } = useI18n();
const { visibleFiles, roots, loadedDirs, searchText, openEditor } = useAppStore();
const cardHost = ref<HTMLElement | null>(null);
const focusedPath = ref("");
const emptyDescription = computed(() => {
  if (loadedDirs.value.length === 0) return t("fileManager.emptyRoot");
  if (roots.value.length === 0) return t("fileManager.noDirectoriesEnabled");
  if (searchText.value.trim()) return t("fileManager.noMatchingFiles");
  return t("fileManager.empty");
});

useListKeyboard({
  host: cardHost,
  rowCount: () => visibleFiles.value.length,
  activeIndex: () => visibleFiles.value.findIndex((file) => file.path === focusedPath.value),
  rowSelector: ".schematic-card",
  onSelect: (index) => {
    const file = visibleFiles.value[index];
    if (file) focusedPath.value = file.path;
  },
  onActivate: (index) => {
    const file = visibleFiles.value[index];
    if (file) void openEditor(file);
  },
  paged: () => false,
});
</script>

<template>
  <section class="schematic-cards">
    <a-scrollbar type="embed" class="scroll-fill__inner" outer-class="scroll-fill">
      <div ref="cardHost" class="schematic-cards__grid" tabindex="0">
        <SchematicCard
          v-for="file in visibleFiles"
          :key="file.path"
          :file="file"
          :focused="focusedPath === file.path"
          @select="focusedPath = $event"
          @edit="openEditor"
        />
      </div>
      <a-empty v-if="!visibleFiles.length" :description="emptyDescription" />
    </a-scrollbar>
  </section>
</template>
