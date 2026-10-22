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
  <div class="toolbox-view">
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
      <div class="tool-list">
        <button
          v-for="tool in TOOLBOX_TOOLS"
          :key="tool.id"
          class="tool-card"
          @click="activeId = tool.id"
        >
          <span class="tool-icon">
            <el-icon :size="22"><component :is="tool.icon" /></el-icon>
          </span>
          <span class="tool-info">
            <span class="tool-name">{{ t(tool.titleKey) }}</span>
            <span class="tool-desc">{{ t(tool.descKey) }}</span>
          </span>
        </button>
      </div>
      <el-empty v-if="TOOLBOX_TOOLS.length === 0" :description="t('toolbox.emptyHint')" :image-size="60" />
    </el-scrollbar>

    <!-- 详情：注册表中的工具组件 -->
    <component :is="activeComponent" v-else :key="activeId ?? 'tool'" />
  </div>
</template>

<style scoped>
.toolbox-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

.tool-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 4px 0;
}

.tool-card {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  background: #ffffff;
  border: 1px solid #eef0f3;
  border-radius: 12px;
  cursor: pointer;
  text-align: left;
  transition: all 0.15s ease;
}

.tool-card:hover {
  border-color: #c6e2ff;
  box-shadow: 0 4px 12px rgba(64, 158, 255, 0.12);
}

.tool-card:active {
  transform: scale(0.98);
}

.tool-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 38px;
  height: 38px;
  border-radius: 10px;
  background: #ecf5ff;
  color: #409eff;
  flex-shrink: 0;
}

.tool-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.tool-name {
  font-size: 13px;
  font-weight: 500;
  color: #1f2937;
}

.tool-desc {
  font-size: 11px;
  color: #9ca3af;
}
</style>
