<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Delete, Plus } from "@element-plus/icons-vue";
import { useNotesStore } from "@/stores/notes";

const { t } = useI18n();
const store = useNotesStore();
const palette = ["#fef3c7", "#dcfce7", "#dbeafe", "#fce7f3", "#ede9fe"];
</script>

<template>
  <div class="notes-view">
    <div class="view-toolbar">
      <span class="toolbar-title">{{ t("notes.title") }}</span>
      <el-button size="small" text type="primary" :icon="Plus" @click="store.addNote()">
        {{ t("notes.new") }}
      </el-button>
    </div>
    <el-scrollbar class="view-scroll">
      <el-empty v-if="store.notes.length === 0" :description="t('notes.empty')" :image-size="60" />
      <div v-for="note in store.notes" :key="note.id" class="note-card" :style="{ background: note.color }">
        <div class="note-actions">
          <div class="note-colors">
            <span
              v-for="c in palette"
              :key="c"
              class="color-dot"
              :class="{ active: note.color === c }"
              :style="{ background: c }"
              @click="store.setNoteColor(note.id, c)"
            ></span>
          </div>
          <el-button size="small" text :icon="Delete" @click="store.removeNote(note.id)" />
        </div>
        <el-input
          :model-value="note.content"
          type="textarea"
          :autosize="{ minRows: 2, maxRows: 6 }"
          :placeholder="t('notes.ph')"
          @update:model-value="(v: string) => store.updateNote(note.id, v)"
        />
      </div>
    </el-scrollbar>
  </div>
</template>

<style scoped>
.notes-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

.note-card {
  border-radius: 10px;
  padding: 8px 10px 10px;
  margin-bottom: 10px;
  box-shadow: 0 2px 8px rgba(31, 41, 55, 0.08);
}

.note-actions {
  display: flex;
  align-items: center;
  margin-bottom: 2px;
}

.note-colors {
  display: flex;
  gap: 5px;
  margin-right: auto;
}

.color-dot {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  cursor: pointer;
  border: 1px solid rgba(31, 41, 55, 0.15);
}

.color-dot.active {
  border: 2px solid #409eff;
}

.note-card :deep(.el-textarea__inner) {
  background: rgba(255, 255, 255, 0.65);
  border-radius: 8px;
  font-size: 13px;
  box-shadow: none;
}
</style>
