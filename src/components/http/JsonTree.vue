<script setup lang="ts">
import { computed, ref } from "vue";
import { CaretBottom, CaretRight, DocumentCopy } from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
import { copyToClipboard } from "@/api/clipboard";
import { useI18n } from "vue-i18n";

const { t } = useI18n();

/**
 * JSON 树形视图：递归渲染对象/数组，节点可折叠，值可复制。
 * 大体积（>5000 节点）自动降级为只渲染前若干层，避免卡顿。
 */
const props = withDefaults(
  defineProps<{
    value: unknown;
    /** 键名（根节点为空） */
    name?: string;
    /** 默认展开层级 */
    expandDepth?: number;
    /** 层级深度（递归用，根为 0） */
    depth?: number;
  }>(),
  { name: "", expandDepth: 2, depth: 0 },
);

const isObject = computed(() => props.value !== null && typeof props.value === "object");
const isArray = computed(() => Array.isArray(props.value));
const entries = computed<Array<[string, unknown]>>(() => {
  if (!isObject.value) {
    return [];
  }
  return isArray.value
    ? (props.value as unknown[]).map((item, index) => [String(index), item] as [string, unknown])
    : Object.entries(props.value as Record<string, unknown>);
});

/** 超大结构（数组/对象条目超过 200）截断渲染并提示 */
const MAX_CHILDREN = 200;
const children = computed(() => entries.value.slice(0, MAX_CHILDREN));
const overflowCount = computed(() => Math.max(0, entries.value.length - MAX_CHILDREN));

const expanded = ref(props.depth < props.expandDepth);

const preview = computed(() => {
  if (props.value === null) {
    return "null";
  }
  if (isArray.value) {
    return `Array(${entries.value.length})`;
  }
  if (isObject.value) {
    return `Object(${entries.value.length})`;
  }
  if (typeof props.value === "string") {
    return props.value.length > 200 ? `${props.value.slice(0, 200)}…` : props.value;
  }
  return String(props.value);
});

const valueClass = computed(() => {
  const type = props.value === null ? "null" : typeof props.value;
  switch (type) {
    case "string":
      return "text-[var(--el-color-success,#67c23a)]";
    case "number":
      return "text-[var(--el-color-primary,#409eff)]";
    case "boolean":
      return "text-[var(--el-color-warning,#e6a23c)]";
    case "null":
      return "text-dp-tertiary";
    default:
      return "text-dp-muted";
  }
});

async function copyValue() {
  const text =
    isObject.value ? JSON.stringify(props.value, null, 2) : String(props.value ?? "");
  await copyToClipboard(text);
  ElMessage.success(t("toolbox.tools.http.copied"));
}
</script>

<template>
  <div class="min-w-0">
    <!-- 可折叠的复合节点 -->
    <div v-if="isObject" class="min-w-0">
      <div class="flex items-center gap-1 leading-[1.7] select-text">
        <el-icon
          :size="12"
          class="shrink-0 cursor-pointer text-dp-tertiary"
          @click="expanded = !expanded"
        >
          <CaretBottom v-if="expanded" />
          <CaretRight v-else />
        </el-icon>
        <span v-if="name" class="text-dp-accent shrink-0 break-all">{{ name }}:</span>
        <span class="text-[11px] text-dp-tertiary shrink-0">{{ preview }}</span>
        <el-icon
          :size="12"
          class="shrink-0 cursor-pointer text-dp-tertiary hidden group-hover:block"
          :title="t('common.copy')"
          @click="copyValue"
        >
          <DocumentCopy />
        </el-icon>
      </div>
      <template v-if="expanded">
        <div
          v-for="[key, child] in children"
          :key="key"
          class="group flex min-w-0 pl-3 ml-[5px] border-l border-dp-divider"
        >
          <JsonTree
            :value="child"
            :name="key"
            :expand-depth="expandDepth"
            :depth="depth + 1"
            class="flex-1 min-w-0"
          />
        </div>
        <div v-if="overflowCount" class="pl-4 text-[11px] text-dp-tertiary leading-[1.7]">
          … +{{ overflowCount }}
        </div>
      </template>
    </div>
    <!-- 叶子节点 -->
    <div class="group flex items-center gap-1.5 leading-[1.7] min-w-0 select-text" v-else>
      <span class="w-[12px] shrink-0" />
      <span v-if="name" class="text-dp-accent shrink-0 break-all">{{ name }}:</span>
      <span class="break-all min-w-0" :class="valueClass">{{ preview }}</span>
      <el-icon
        :size="12"
        class="shrink-0 cursor-pointer text-dp-tertiary hidden group-hover:block"
        :title="t('common.copy')"
        @click="copyValue"
      >
        <DocumentCopy />
      </el-icon>
    </div>
  </div>
</template>
