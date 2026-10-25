<script setup lang="ts">
import { computed, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useAppStore } from "../store";

const { t } = useI18n();
const { analysis, analyzing, activeRegion, setActiveRegion } = useAppStore();
const regions = computed(() => analysis.value?.regions ?? []);

watch(regions, (items) => {
  if (activeRegion.value && !items.some((item) => item.name === activeRegion.value)) {
    setActiveRegion(null);
  }
});

function triple(values: [number, number, number]): string {
  return values.join(" × ");
}
</script>

<template>
  <div class="region-list">
    <div class="region-list__title">{{ t("regions.title") }}</div>
    <a-spin :loading="analyzing" class="region-list__spin">
      <a-scrollbar type="embed" class="scroll-fill__inner" outer-class="scroll-fill">
        <div v-if="analysis" class="region-list__items">
          <button
            class="region-list__item"
            :class="{ 'region-list__item--active': activeRegion === null }"
            @click="setActiveRegion(null)"
          >
            {{ t("regions.all") }}
          </button>
          <button
            v-for="region in regions"
            :key="region.name"
            class="region-list__item"
            :class="{ 'region-list__item--active': activeRegion === region.name }"
            :title="`${t('regions.position', { position: triple(region.position) })} · ${t('regions.size', { size: triple(region.size) })}`"
            @click="setActiveRegion(region.name)"
          >
            <span class="region-list__name">{{ region.name }}</span>
            <span class="region-list__detail">{{ t("regions.size", { size: triple(region.size) }) }}</span>
            <span class="region-list__detail">{{ t("regions.position", { position: triple(region.position) }) }}</span>
          </button>
          <a-empty v-if="regions.length === 0" :description="t('regions.empty')" />
        </div>
        <a-empty v-else :description="t('detail.empty')" />
      </a-scrollbar>
    </a-spin>
    <div class="region-list__note hint-dim">{{ t("regions.wholeFileNote") }}</div>
  </div>
</template>
