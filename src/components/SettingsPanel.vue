<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { LOCALES, currentLocale, setLocale } from "../i18n";
import { useAppStore } from "../store";

const { t } = useI18n();
const { openBackups, openSetup, openUpdate, setupOnOpen } = useAppStore();
const language = computed({
  get: () => currentLocale(),
  set: (value: string) => setLocale(value as (typeof LOCALES)[number]["locale"]),
});
</script>

<template>
  <div class="settings-panel">
    <div class="settings-panel__intro">
      <h2>{{ t("settings.title") }}</h2>
      <p class="hint-dim">{{ t("settings.description") }}</p>
    </div>
    <div class="settings-panel__grid">
      <section class="settings-card">
        <h3>{{ t("settings.language") }}</h3>
        <a-select v-model="language" style="width: 220px">
          <a-option v-for="item in LOCALES" :key="item.locale" :value="item.locale">
            {{ t(item.labelKey) }}
          </a-option>
        </a-select>
      </section>
      <section class="settings-card">
        <h3>{{ t("settings.versionLibrary") }}</h3>
        <a-button type="primary" :loading="setupOnOpen" @click="openSetup">
          {{ t("settings.open") }}
        </a-button>
      </section>
      <section class="settings-card">
        <h3>{{ t("settings.backupsLogs") }}</h3>
        <a-button @click="openBackups">{{ t("settings.open") }}</a-button>
      </section>
      <section class="settings-card">
        <h3>{{ t("settings.update") }}</h3>
        <a-button @click="openUpdate">{{ t("settings.open") }}</a-button>
      </section>
    </div>
  </div>
</template>
