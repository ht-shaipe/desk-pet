<script setup lang="ts">
import { computed, reactive, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { Delete, EditPen, Plus, Setting } from "@element-plus/icons-vue";
import { launchItem } from "@/api";
import { useLauncherStore } from "@/stores/launcher";
import { useConfigStore } from "@/stores/config";
import type { LaunchType, LauncherItem } from "@/types";

const { t } = useI18n();
const store = useLauncherStore();
const config = useConfigStore();
const editMode = ref(false);
const dialogVisible = ref(false);
const editingId = ref<string | null>(null);

function emptyForm(): LauncherItem {
  return {
    id: "",
    name: "",
    icon: "",
    item_type: "app",
    value: "",
    color: store.nextColor(),
  };
}

const form = reactive<LauncherItem>(emptyForm());

const typeOptions = computed<Array<{ label: string; value: LaunchType }>>(() => [
  { label: t("launcher.typeApp"), value: "app" },
  { label: t("launcher.typeUrl"), value: "url" },
  { label: t("launcher.typeCmd"), value: "command" },
]);

const valuePlaceholder = computed(() => {
  const mac = config.platform !== "windows";
  switch (form.item_type) {
    case "url":
      return t("launcher.ph.url");
    case "command":
      return mac ? t("launcher.ph.cmdMac") : t("launcher.ph.cmdWin");
    default:
      return mac ? t("launcher.ph.appMac") : t("launcher.ph.appWin");
  }
});

async function onLaunch(item: LauncherItem) {
  if (editMode.value) {
    return;
  }
  try {
    await launchItem(item);
  } catch (e) {
    ElMessage.error(t("launcher.launchFail", { err: String(e) }));
  }
}

function openAdd() {
  editingId.value = null;
  Object.assign(form, emptyForm());
  dialogVisible.value = true;
}

function openEdit(item: LauncherItem) {
  editingId.value = item.id;
  Object.assign(form, item);
  dialogVisible.value = true;
}

function confirmDialog() {
  if (!form.name.trim() || !form.value.trim()) {
    ElMessage.warning(t("launcher.emptyWarn"));
    return;
  }
  if (editingId.value) {
    store.updateItem({ ...form, id: editingId.value });
    ElMessage.success(t("common.saved"));
  } else {
    store.addItem({
      ...form,
      id: `l-${Date.now()}-${Math.floor(Math.random() * 1000)}`,
      name: form.name.trim(),
      value: form.value.trim(),
    });
    ElMessage.success(t("common.added"));
  }
  dialogVisible.value = false;
}
</script>

<template>
  <div class="launch-view">
    <div class="view-toolbar">
      <span class="toolbar-title">{{ t("launcher.title") }}</span>
      <el-button v-if="!editMode" size="small" text :icon="Setting" @click="editMode = true">
        {{ t("launcher.manage") }}
      </el-button>
      <el-button v-else size="small" text type="primary" @click="editMode = false">
        {{ t("launcher.done") }}
      </el-button>
    </div>
    <el-scrollbar class="view-scroll">
      <div v-if="store.items.length > 0 || editMode" class="launch-grid">
        <div
          v-for="item in store.items"
          :key="item.id"
          class="launch-item"
          @click="editMode ? openEdit(item) : onLaunch(item)"
        >
          <span v-if="editMode" class="item-delete" @click.stop="store.removeItem(item.id)">
            <el-icon :size="8"><Delete /></el-icon>
          </span>
          <span class="item-avatar" :style="{ background: item.color || '#909399' }">
            {{ item.icon || item.name.slice(0, 1) }}
          </span>
          <span class="item-name" :title="item.name">{{ item.name }}</span>
        </div>
        <div v-if="editMode" class="launch-item add" @click="openAdd">
          <span class="item-avatar add-avatar">
            <el-icon :size="18"><Plus /></el-icon>
          </span>
          <span class="item-name">{{ t("launcher.add") }}</span>
        </div>
      </div>
      <el-empty
        v-if="store.items.length === 0 && !editMode"
        :description="t('launcher.empty')"
        :image-size="60"
      />
      <div v-if="editMode" class="edit-hint">
        <el-icon :size="12"><EditPen /></el-icon>
        {{ t("launcher.editHint") }}
      </div>
    </el-scrollbar>

    <el-dialog
      v-model="dialogVisible"
      :title="editingId ? t('launcher.editTitle') : t('launcher.addTitle')"
      width="300"
      append-to-body
    >
      <el-form label-position="top" size="small">
        <el-form-item :label="t('launcher.name')">
          <el-input v-model="form.name" :placeholder="t('launcher.namePh')" maxlength="20" />
        </el-form-item>
        <el-form-item :label="t('launcher.type')">
          <el-select v-model="form.item_type">
            <el-option v-for="opt in typeOptions" :key="opt.value" :label="opt.label" :value="opt.value" />
          </el-select>
        </el-form-item>
        <el-form-item :label="t('launcher.value')">
          <el-input v-model="form.value" :placeholder="valuePlaceholder" />
        </el-form-item>
        <el-form-item :label="t('launcher.iconLabel')">
          <el-input v-model="form.icon" maxlength="2" :placeholder="t('launcher.iconPh')" />
        </el-form-item>
        <el-form-item :label="t('launcher.color')">
          <el-color-picker v-model="form.color" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button size="small" @click="dialogVisible = false">{{ t("common.cancel") }}</el-button>
        <el-button size="small" type="primary" @click="confirmDialog">{{ t("common.confirm") }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.launch-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

.launch-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 10px;
  padding: 8px 2px 2px;
}

.launch-item {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 8px 2px;
  border-radius: 10px;
  cursor: pointer;
  transition: background 0.15s ease;
}

.launch-item:hover {
  background: #eef1f5;
}

.item-avatar {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 42px;
  height: 42px;
  border-radius: 50%;
  color: #ffffff;
  font-size: 17px;
  font-weight: 600;
  overflow: hidden;
}

.add-avatar {
  background: #e3e6ea;
  color: #6b7280;
}

.item-name {
  max-width: 100%;
  font-size: 12px;
  color: #374151;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.item-delete {
  position: absolute;
  top: 2px;
  right: 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: #f56c6c;
  color: #ffffff;
  cursor: pointer;
}

.edit-hint {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-top: 10px;
  color: #9ca3af;
  font-size: 12px;
}
</style>
