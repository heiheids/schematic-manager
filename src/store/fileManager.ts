import { computed, ref, watch } from "vue";
import type { ComputedRef, Ref } from "vue";
import type { DirectoryNode, SchematicSummary } from "../types";

export interface FileManagerDeps {
  dirTrees: Ref<Record<string, DirectoryNode>>;
  enabledDirs: Ref<Set<string>>;
  filteredFiles: ComputedRef<SchematicSummary[]>;
  checkedPaths: Ref<Set<string>>;
}

export interface DirectoryView {
  root: string;
  node: DirectoryNode;
  key: string;
}

export function directoryKey(root: string, node: DirectoryNode): string {
  return `${root}\u0000${node.relativePath}`;
}

export function collectDirectoryFiles(node: DirectoryNode, recursive: boolean): Set<string> {
  const paths = new Set<string>(node.filePaths);
  if (recursive) {
    for (const child of node.children) {
      for (const path of collectDirectoryFiles(child, true)) paths.add(path);
    }
  }
  return paths;
}

export function createFileManagerSlice(s: FileManagerDeps) {
  const selectedDirKey = ref("");
  const expandedDirKeys = ref<Set<string>>(new Set());
  const showDescendants = ref(false);

  const roots = computed<DirectoryView[]>(() =>
    Object.entries(s.dirTrees.value)
      .filter(([root, node]) => s.enabledDirs.value.has(root) && Boolean(node))
      .map(([root, node]) => ({ root, node, key: directoryKey(root, node) })),
  );

  function findNode(key: string): DirectoryView | null {
    function visit(root: string, node: DirectoryNode): DirectoryView | null {
      const current = directoryKey(root, node);
      if (current === key) return { root, node, key: current };
      for (const child of node.children) {
        const hit = visit(root, child);
        if (hit) return hit;
      }
      return null;
    }
    for (const item of roots.value) {
      const hit = visit(item.root, item.node);
      if (hit) return hit;
    }
    return null;
  }

  const selectedDirectory = computed(() => findNode(selectedDirKey.value));

  watch(
    roots,
    (items) => {
      if (items.length === 0) {
        selectedDirKey.value = "";
        return;
      }
      if (!selectedDirectory.value) selectedDirKey.value = items[0].key;
      const expanded = new Set(expandedDirKeys.value);
      for (const item of items) {
        if (!expandedDirKeys.value.has(item.key)) expanded.add(item.key);
      }
      expandedDirKeys.value = expanded;
    },
    { immediate: true, deep: false },
  );

  function selectDirectory(key: string) {
    if (findNode(key)) selectedDirKey.value = key;
  }

  function toggleDirectory(key: string) {
    const next = new Set(expandedDirKeys.value);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    expandedDirKeys.value = next;
  }

  function isExpanded(key: string): boolean {
    return expandedDirKeys.value.has(key);
  }

  const visibleFiles = computed(() => {
    const current = selectedDirectory.value;
    if (!current) return [];
    const paths = collectDirectoryFiles(current.node, showDescendants.value);
    return s.filteredFiles.value.filter((file) => paths.has(file.path));
  });

  const visibleCheckedFiles = computed(() =>
    visibleFiles.value.filter((file) => s.checkedPaths.value.has(file.path)),
  );
  const visibleCheckedCount = computed(() => visibleCheckedFiles.value.length);

  function setFileChecked(path: string, checked: boolean) {
    const next = new Set(s.checkedPaths.value);
    if (checked) next.add(path);
    else next.delete(path);
    s.checkedPaths.value = next;
  }

  function checkAllVisible() {
    const next = new Set(s.checkedPaths.value);
    for (const file of visibleFiles.value) next.add(file.path);
    s.checkedPaths.value = next;
  }

  function invertVisible() {
    const next = new Set(s.checkedPaths.value);
    for (const file of visibleFiles.value) {
      if (next.has(file.path)) next.delete(file.path);
      else next.add(file.path);
    }
    s.checkedPaths.value = next;
  }

  function clearVisible() {
    const next = new Set(s.checkedPaths.value);
    for (const file of visibleFiles.value) next.delete(file.path);
    s.checkedPaths.value = next;
  }

  return {
    roots,
    selectedDirKey,
    selectedDirectory,
    expandedDirKeys,
    showDescendants,
    visibleFiles,
    visibleCheckedFiles,
    visibleCheckedCount,
    selectDirectory,
    toggleDirectory,
    isExpanded,
    setFileChecked,
    checkAllVisible,
    invertVisible,
    clearVisible,
  };
}

export type FileManagerSlice = ReturnType<typeof createFileManagerSlice>;
