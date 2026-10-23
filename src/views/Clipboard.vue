<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { Delete, Link, Search } from "@element-plus/icons-vue";
import { useClipboardStore } from "@/stores/clipboard";

const { t } = useI18n();
const store = useClipboardStore();
const keyword = ref("");

/** 识别 URL 类内容，链接文本用主题色呈现。 */
function isUrl(text: string): boolean {
  return /^https?:\/\//i.test(text.trim());
}

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
  <div class="view-root [&_.view-toolbar]:gap-1.5">
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
        class="clip-item relative bg-dp-card-bg border border-dp-card-border rounded-[10px] pt-2 pr-2.5 pb-1.5 pl-3 mb-1.5 cursor-pointer overflow-hidden transition-[border-color,box-shadow] duration-150 ease-[ease] hover:border-dp-card-hover-border hover:shadow-dp-card-hover before:content-[''] before:absolute before:left-0 before:top-0 before:bottom-0 before:w-[3px] before:bg-dp-accent before:opacity-0 before:transition-opacity before:duration-150 before:ease-[ease] hover:before:opacity-100"
        :title="t('clipboard.copyTip')"
        @click="onCopy(entry.text)"
      >
        <div class="flex items-start gap-[5px] text-xs text-dp-secondary leading-[1.5]">
          <el-icon v-if="isUrl(entry.text)" class="shrink-0 mt-0.5 text-dp-accent"><Link /></el-icon>
          <span
            class="whitespace-pre-wrap break-all line-clamp-2"
            :class="isUrl(entry.text) ? 'text-dp-accent' : ''"
          >{{ entry.text }}</span>
        </div>
        <div class="flex items-center justify-between mt-[5px] text-[11px] text-dp-tertiary">
          <span class="inline-flex items-center gap-[5px] min-w-0">
            <span v-if="isUrl(entry.text)" class="shrink-0 text-[10px] leading-none py-0.5 px-[5px] rounded-[4px] text-dp-accent bg-dp-tag-soft">URL</span>
            <span v-else-if="entry.text.length > 80" class="shrink-0 text-[10px] leading-none py-0.5 px-[5px] rounded-[4px] text-dp-accent bg-dp-tag-soft">TEXT</span>
            {{ timeText(entry.updated_at) }}
          </span>
          <span
            class="inline-flex items-center p-[3px] rounded-[5px] opacity-0 transition-opacity duration-150 ease-[ease] [.clip-item:hover_&]:opacity-100 hover:text-[#f56c6c] hover:bg-dp-danger-soft"
            :title="t('common.delete')"
            @click.stop="store.remove(entry.id)"
          >
            <el-icon :size="12"><Delete /></el-icon>
          </span>
        </div>
      </div>
    </el-scrollbar>
  </div>
</template>
