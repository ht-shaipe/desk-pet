<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ArrowLeft } from "@element-plus/icons-vue";
import { TOOLBOX_TOOLS } from "@/components/toolbox/registry";

const { t } = useI18n();

/** 当前打开的工具 ID；null 表示工具箱首页 */
const activeId = ref<string | null>(null);

const activeTool = computed(() => TOOLBOX_TOOLS.find((tool) => tool.id === activeId.value) ?? null);
const activeComponent = computed(() => activeTool.value?.component);
</script>

<template>
  <div class="view-root">
    <div class="view-toolbar">
      <el-button
        v-if="activeTool"
        size="small"
        text
        :icon="ArrowLeft"
        @click="activeId = null"
      >
        {{ t("toolbox.back") }}
      </el-button>
      <span class="toolbar-title">
        {{ activeTool ? t(activeTool.titleKey) : t("toolbox.title") }}
      </span>
    </div>

    <!-- 首页：工具卡片列表 -->
    <el-scrollbar v-if="!activeTool" class="view-scroll">
      <div class="grid grid-cols-2 gap-2 py-1">
        <button
          v-for="tool in TOOLBOX_TOOLS"
          :key="tool.id"
          class="flex items-center gap-2 p-2.5 bg-dp-card-bg border border-dp-card-border rounded-xl cursor-pointer text-left transition-all duration-150 ease-[ease] hover:border-dp-card-hover-border hover:shadow-dp-card-hover active:scale-[0.98]"
          @click="activeId = tool.id"
        >
          <span
            class="inline-flex items-center justify-center w-8 h-8 rounded-lg bg-dp-tag-soft text-dp-accent shrink-0"
          >
            <el-icon :size="18"><component :is="tool.icon" /></el-icon>
          </span>
          <span class="flex flex-col gap-0.5 min-w-0">
            <span class="text-xs font-medium text-dp-primary">{{ t(tool.titleKey) }}</span>
            <span class="text-[10px] text-dp-tertiary truncate">{{ t(tool.descKey) }}</span>
          </span>
        </button>
      </div>
      <el-empty v-if="TOOLBOX_TOOLS.length === 0" :description="t('toolbox.emptyHint')" :image-size="60" />
    </el-scrollbar>

    <!-- 详情：注册表中的工具组件 -->
    <component :is="activeComponent" v-else :key="activeId ?? 'tool'" />
  </div>
</template>
