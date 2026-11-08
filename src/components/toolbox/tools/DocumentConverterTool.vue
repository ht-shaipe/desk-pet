<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { Document, FolderOpened, Loading } from "@element-plus/icons-vue";
import { docsOpen, docsListDirs } from "@/api/docs";
import type { DocDirInfo } from "@/types";

const { t } = useI18n();

const dirs = ref<DocDirInfo[]>([]);
const opening = ref(false);

async function openManager() {
  opening.value = true;
  try {
    await docsOpen();
  } catch (e) {
    ElMessage.error(String(e));
  } finally {
    opening.value = false;
  }
}

onMounted(() => {
  docsListDirs()
    .then((d) => (dirs.value = d))
    .catch(() => undefined);
});
</script>

<template>
  <div class="view-root">
    <el-scrollbar class="view-scroll">
      <div class="p-2.5 bg-dp-card-bg border border-dp-card-border rounded-[10px]">
        <div class="text-xs text-dp-muted leading-[1.7]">
          {{ t("toolbox.tools.docconv.intro") }}
        </div>
        <el-button
          type="primary"
          size="small"
          class="mt-2.5 w-full"
          :loading="opening"
          :icon="FolderOpened"
          @click="openManager"
        >
          {{ t("toolbox.tools.docconv.open") }}
        </el-button>
      </div>
      <div v-if="dirs.length" class="mt-3 px-1">
        <div class="text-[11px] text-dp-tertiary mb-1.5">{{ t("toolbox.tools.docconv.libraries") }}</div>
        <div
          v-for="d in dirs"
          :key="d.id"
          class="flex items-center gap-1.5 py-1 text-[11px] text-dp-muted min-w-0"
        >
          <el-icon :size="11" class="shrink-0"><Document /></el-icon>
          <span class="truncate flex-1 min-w-0" :title="d.path">{{ d.path }}</span>
          <span class="shrink-0 text-dp-tertiary">{{ d.item_count }}</span>
          <el-icon v-if="!d.last_scan_at" class="shrink-0 text-dp-tertiary" :size="11" :title="t('toolbox.tools.docconv.notScanned')">
            <Loading />
          </el-icon>
        </div>
      </div>
    </el-scrollbar>
  </div>
</template>
