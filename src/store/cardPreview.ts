import { onBeforeUnmount, ref } from "vue";
import { buildScene } from "../api";
import { OffscreenRenderer } from "../render/scene3d";
import { SCENE_BLOCK_LIMIT } from "./scenePreview";
import type { SchematicSummary } from "../types";

export type CardPreviewStatus = "idle" | "loading" | "ready" | "error" | "gated";

export interface CardPreview {
  status: CardPreviewStatus;
  url: string;
  error: string;
}

const EMPTY: CardPreview = { status: "idle", url: "", error: "" };

export function createCardPreviewSlice() {
  const values = ref<Record<string, CardPreview>>({});
  const records = new Map<string, SchematicSummary>();
  const queued = new Set<string>();
  const queue: string[] = [];
  let renderer: OffscreenRenderer | null = null;
  let running = false;
  let generation = 0;
  const pathGenerations = new Map<string, number>();

  function previewOf(path: string): CardPreview {
    return values.value[path] ?? EMPTY;
  }

  function revoke(path: string) {
    const url = values.value[path]?.url;
    if (url && typeof URL.revokeObjectURL === "function") URL.revokeObjectURL(url);
  }

  function setPreview(path: string, value: CardPreview) {
    revoke(path);
    values.value = { ...values.value, [path]: value };
  }

  async function pump() {
    if (running) return;
    running = true;
    try {
      while (queue.length > 0) {
        const path = queue.shift()!;
        queued.delete(path);
        const record = records.get(path);
        const gen = generation;
        const pathGeneration = pathGenerations.get(path) ?? 0;
        if (!record) continue;
        setPreview(path, { status: "loading", url: "", error: "" });
        try {
          if (!renderer) renderer = new OffscreenRenderer();
          const scene = await buildScene(path);
          if (
            gen !== generation ||
            pathGeneration !== (pathGenerations.get(path) ?? 0) ||
            !records.has(path)
          ) {
            continue;
          }
          const bytes = await renderer.renderPng(scene, 320, 240, "isoLeft");
          if (
            gen !== generation ||
            pathGeneration !== (pathGenerations.get(path) ?? 0) ||
            !records.has(path)
          ) {
            continue;
          }
          const url = URL.createObjectURL(new Blob([bytes], { type: "image/png" }));
          setPreview(path, { status: "ready", url, error: "" });
        } catch (error) {
          if (
            gen === generation &&
            pathGeneration === (pathGenerations.get(path) ?? 0) &&
            records.has(path)
          ) {
            setPreview(path, { status: "error", url: "", error: String(error) });
          }
        }
      }
    } finally {
      running = false;
    }
  }

  function requestPreview(record: SchematicSummary, allowHeavy = false) {
    const path = record.path;
    records.set(path, record);
    const current = previewOf(path);
    if (record.error) {
      setPreview(path, { status: "error", url: "", error: record.error });
      return;
    }
    if (record.totalBlocks > SCENE_BLOCK_LIMIT && !allowHeavy) {
      if (current.status !== "ready" && current.status !== "loading") {
        setPreview(path, { status: "gated", url: "", error: "" });
      }
      return;
    }
    if (current.status === "ready" || current.status === "loading" || queued.has(path)) return;
    queued.add(path);
    queue.push(path);
    void pump();
  }

  function invalidate(paths?: string[]) {
    const selected = paths ? new Set(paths) : null;
    if (selected) {
      for (const path of selected) {
        pathGenerations.set(path, (pathGenerations.get(path) ?? 0) + 1);
      }
    } else {
      generation++;
    }
    for (const path of Object.keys(values.value)) {
      if (!selected || selected.has(path)) revoke(path);
    }
    values.value = Object.fromEntries(
      Object.entries(values.value).filter(([path]) => selected && !selected.has(path)),
    );
    for (const path of [...records.keys()]) {
      if (!selected || selected.has(path)) records.delete(path);
    }
    for (let i = queue.length - 1; i >= 0; i--) {
      if (!selected || selected.has(queue[i])) {
        queued.delete(queue[i]);
        queue.splice(i, 1);
      }
    }
  }

  onBeforeUnmount(() => {
    generation++;
    for (const path of Object.keys(values.value)) revoke(path);
    values.value = {};
    records.clear();
    queue.length = 0;
    queued.clear();
    pathGenerations.clear();
    renderer?.dispose();
    renderer = null;
  });

  return { previewOf, requestPreview, invalidate, invalidateCardPreviews: invalidate };
}

export type CardPreviewSlice = ReturnType<typeof createCardPreviewSlice>;
