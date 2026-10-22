<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Delete, Plus } from "@element-plus/icons-vue";
import { useNotesStore } from "@/stores/notes";

const { t } = useI18n();
const store = useNotesStore();
const palette = ["#fef3c7", "#dcfce7", "#dbeafe", "#fce7f3", "#ede9fe"];
</script>

<template>
  <div class="view-root">
    <div class="view-toolbar">
      <span class="toolbar-title">{{ t("notes.title") }}</span>
      <el-button size="small" text type="primary" :icon="Plus" @click="store.addNote()">
        {{ t("notes.new") }}
      </el-button>
    </div>
    <el-scrollbar class="view-scroll">
      <el-empty v-if="store.notes.length === 0" :description="t('notes.empty')" :image-size="60" />
      <div
        v-for="note in store.notes"
        :key="note.id"
        class="rounded-[10px] pt-2 px-2.5 pb-2.5 mb-2.5 shadow-dp-card dark:bg-[linear-gradient(rgba(15,17,21,0.55),rgba(15,17,21,0.55))]"
        :style="{ backgroundColor: note.color }"
      >
        <div class="flex items-center mb-0.5">
          <div class="flex gap-[5px] mr-auto">
            <span
              v-for="c in palette"
              :key="c"
              class="w-3 h-3 rounded-full cursor-pointer"
              :class="note.color === c ? 'border-2 border-dp-accent' : 'border border-dp-dot-border'"
              :style="{ background: c }"
              @click="store.setNoteColor(note.id, c)"
            ></span>
          </div>
          <el-button size="small" text :icon="Delete" @click="store.removeNote(note.id)" />
        </div>
        <el-input
          class="[&_.el-textarea\_\_inner]:bg-dp-note-input-bg [&_.el-textarea\_\_inner]:rounded-lg [&_.el-textarea\_\_inner]:text-[13px] [&_.el-textarea\_\_inner]:shadow-none"
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
