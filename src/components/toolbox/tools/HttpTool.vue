<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { Connection } from "@element-plus/icons-vue";
import { httpOpen } from "@/api/toolbox";

const { t } = useI18n();

const opening = ref(false);

async function openWindow() {
  opening.value = true;
  try {
    await httpOpen();
  } catch (e) {
    ElMessage.error(String(e));
  } finally {
    opening.value = false;
  }
}
</script>

<template>
  <div class="view-root">
    <el-scrollbar class="view-scroll">
      <div class="p-2.5 bg-dp-card-bg border border-dp-card-border rounded-[10px]">
        <div class="text-xs text-dp-muted leading-[1.7]">
          {{ t("toolbox.tools.http.intro") }}
        </div>
        <el-button
          type="primary"
          size="small"
          class="mt-2.5 w-full"
          :loading="opening"
          :icon="Connection"
          @click="openWindow"
        >
          {{ t("toolbox.tools.http.open") }}
        </el-button>
      </div>
    </el-scrollbar>
  </div>
</template>
