<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { Delete, Search } from "@element-plus/icons-vue";
import { useClipboardStore } from "@/stores/clipboard";

const { t } = useI18n();
const store = useClipboardStore();
const keyword = ref("");

const filtered = computed(() => {
  const k = keyword.value.trim().toLowerCase();
  if (!k) {
    return store.entries;
  }
  return store.entries.filter((e) => e.text.toLowerCase().includes(k));
});

function timeText(ts: number): string {
  const diff = Date.now() - ts;
  const min = Math.floor(diff / 60000);
  if (min < 1) {
    return t("clipboard.justNow");
  }
  if (min < 60) {
    return t("clipboard.minutesAgo", { n: min });
  }
  const hour = Math.floor(min / 60);
  if (hour < 24) {
    return t("clipboard.hoursAgo", { n: hour });
  }
  return t("clipboard.daysAgo", { n: Math.floor(hour / 24) });
}

async function onCopy(text: string) {
  try {
    await store.copy(text);
    ElMessage.success(t("clipboard.copied"));
  } catch (e) {
    ElMessage.error(t("clipboard.copyFail", { err: String(e) }));
  }
}

async function onClear() {
  await store.clearAll();
  ElMessage.success(t("clipboard.cleared"));
}
</script>

<template>
  <div class="clip-view">
    <div class="view-toolbar">
      <el-input v-model="keyword" size="small" clearable :placeholder="t('clipboard.searchPh')" :prefix-icon="Search" />
      <el-popconfirm
        :title="t('clipboard.clearTitle')"
        :confirm-button-text="t('clipboard.clear')"
        :cancel-button-text="t('common.cancel')"
        @confirm="onClear"
      >
        <template #reference>
          <el-button size="small" text type="danger">{{ t("clipboard.clear") }}</el-button>
        </template>
      </el-popconfirm>
    </div>
    <el-scrollbar class="view-scroll">
      <el-empty v-if="filtered.length === 0" :description="t('clipboard.empty')" :image-size="60" />
      <div
        v-for="entry in filtered"
        :key="entry.id"
        class="clip-item"
        @click="onCopy(entry.text)"
      >
        <div class="clip-text">{{ entry.text }}</div>
        <div class="clip-meta">
          <span>{{ timeText(entry.updated_at) }}</span>
          <span class="clip-delete" :title="t('common.delete')" @click.stop="store.remove(entry.id)">
            <el-icon :size="12"><Delete /></el-icon>
          </span>
        </div>
      </div>
    </el-scrollbar>
  </div>
</template>

<style scoped>
.clip-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

.clip-view .view-toolbar {
  gap: 6px;
}

.clip-item {
  background: #ffffff;
  border: 1px solid #eef0f3;
  border-radius: 10px;
  padding: 8px 10px;
  margin-bottom: 8px;
  cursor: pointer;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}

.clip-item:hover {
  border-color: #c6e2ff;
  box-shadow: 0 2px 8px rgba(64, 158, 255, 0.12);
}

.clip-text {
  font-size: 12px;
  color: #374151;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.clip-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 6px;
  font-size: 11px;
  color: #9ca3af;
}

.clip-delete {
  display: inline-flex;
  align-items: center;
  padding: 2px;
  border-radius: 4px;
}

.clip-delete:hover {
  color: #f56c6c;
  background: #fef0f0;
}
</style>
