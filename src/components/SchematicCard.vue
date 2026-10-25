<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useAppStore } from "../store";
import { formatDims, formatSize } from "../utils/format";
import type { SchematicSummary } from "../types";

const props = withDefaults(defineProps<{ file: SchematicSummary; focused?: boolean }>(), {
  focused: false,
});
const emit = defineEmits<{
  (e: "select", path: string): void;
  (e: "edit", file: SchematicSummary): void;
}>();
const { t } = useI18n();
const {
  checkedPaths,
  setFileChecked,
  requestPreview,
  previewOf,
  isDup,
  isSuspect,
  dupNoOf,
  dupColorNo,
  onRowContextMenu,
  rowClass,
} = useAppStore();

const host = ref<HTMLElement | null>(null);
let observer: IntersectionObserver | null = null;
let visible = false;
const preview = computed(() => previewOf(props.file.path));
const cardClass = computed(() => [
  props.file.error ? "schematic-card--error" : "",
  props.focused ? "schematic-card--focused" : "",
  rowClass(props.file as never),
]);

function loadWhenVisible() {
  requestPreview(props.file);
}

function contextMenu(event: MouseEvent) {
  onRowContextMenu(props.file as never, event);
}

watch(
  () => preview.value.status,
  (status) => {
    if (visible && status === "idle") loadWhenVisible();
  },
);

onMounted(() => {
  if (typeof IntersectionObserver === "undefined") {
    visible = true;
    loadWhenVisible();
    return;
  }
  observer = new IntersectionObserver(
    (entries) => {
      visible = entries.some((entry) => entry.isIntersecting);
      if (visible) loadWhenVisible();
    },
    { rootMargin: "120px" },
  );
  if (host.value) observer.observe(host.value);
});

onBeforeUnmount(() => {
  observer?.disconnect();
  observer = null;
});
</script>

<template>
  <article
    ref="host"
    class="schematic-card"
    :class="cardClass"
    tabindex="0"
    @click="emit('select', file.path)"
    @dblclick="emit('edit', file)"
    @contextmenu="contextMenu"
    @keydown.enter.stop.prevent="emit('edit', file)"
  >
    <div class="schematic-card__preview">
      <img v-if="preview.status === 'ready'" :src="preview.url" :alt="file.fileName" />
      <span v-else-if="preview.status === 'loading'" class="hint-dim">
        {{ t("fileManager.previewLoading") }}
      </span>
      <span v-else-if="preview.status === 'error'" class="schematic-card__error-text" :title="preview.error">
        {{ t("fileManager.previewFailed", { error: preview.error }) }}
      </span>
      <a-button v-else-if="preview.status === 'gated'" size="mini" @click.stop="requestPreview(file, true)">
        {{ t("fileManager.renderPreview") }}
      </a-button>
      <span v-else class="hint-dim">{{ t("fileManager.noPreview") }}</span>
    </div>
    <div class="schematic-card__body">
      <div class="schematic-card__title" :title="file.fileName">
        <span v-if="isDup(file.path)" :class="['dup-chip', `dup-chip-${dupColorNo(file.path)}`]">{{ dupNoOf(file.path) }}</span>
        <span v-else-if="isSuspect(file.path)" class="dup-chip dup-chip--suspect">
          {{ t("files.suspectChip") }}
        </span>
        {{ file.fileName }}
      </div>
      <div class="schematic-card__name" :title="file.name">{{ file.name || file.fileName }}</div>
      <div class="schematic-card__meta">
        <span>{{ file.mcVersionName || t("detail.unknown") }}</span>
        <span>{{ formatDims(file.enclosing) }}</span>
        <span>{{ file.totalBlocks }}</span>
        <span>{{ formatSize(file.fileSize) }}</span>
      </div>
      <div v-if="file.error" class="schematic-card__error-text" :title="file.error">{{ file.error }}</div>
      <div class="schematic-card__actions">
        <a-checkbox
          :model-value="checkedPaths.has(file.path)"
          @change="(value: any) => setFileChecked(file.path, Boolean(value))"
          @click.stop
        />
        <a-button size="mini" @click.stop="emit('edit', file)">{{ t("fileManager.edit") }}</a-button>
      </div>
    </div>
  </article>
</template>
