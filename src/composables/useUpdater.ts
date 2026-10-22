/**
 * 自动更新组合式函数（参照 vortex 实现，适配 desk-pet）。
 * 桌面环境下检查、下载并安装应用更新，提供状态、进度等响应式数据。
 */
import { ref, readonly, defineComponent, h } from "vue";

/** 更新流程状态 */
export type UpdateStatus =
  | "idle"
  | "checking"
  | "available"
  | "up-to-date"
  | "downloading"
  | "ready"
  | "error";

/** 更新信息 */
interface UpdateInfo {
  version: string;
  body: string;
  date: string;
}

const status = ref<UpdateStatus>("idle");
const updateInfo = ref<UpdateInfo | null>(null);
const errorMsg = ref("");
const downloadProgress = ref(0);
const currentVersion = ref("");

// 插件返回的更新对象（懒加载避免开发模式无签名报错）
let update: import("@tauri-apps/plugin-updater").Update | null = null;

/** 检查是否有可用更新，返回是否发现新版本 */
async function checkForUpdate(_silent = false): Promise<boolean> {
  status.value = "checking";
  errorMsg.value = "";
  updateInfo.value = null;
  update = null;
  try {
    const { check } = await import("@tauri-apps/plugin-updater");
    const { getVersion } = await import("@tauri-apps/api/app");
    currentVersion.value = await getVersion();
    const found = await check();
    if (found) {
      update = found;
      updateInfo.value = {
        version: found.version,
        body: found.body || "",
        date: found.date || "",
      };
      status.value = "available";
      return true;
    }
    status.value = "up-to-date";
    return false;
  } catch (e) {
    status.value = "error";
    errorMsg.value = e instanceof Error ? e.message : String(e);
    return false;
  }
}

/** 下载并安装更新，实时更新进度 */
async function downloadAndInstall(): Promise<void> {
  if (!update) {
    status.value = "error";
    errorMsg.value = "没有可用的更新";
    return;
  }
  status.value = "downloading";
  downloadProgress.value = 0;
  try {
    let total = 0;
    let downloaded = 0;
    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case "Started":
          total = event.data.contentLength ?? 0;
          break;
        case "Progress":
          downloaded += event.data.chunkLength;
          if (total > 0) {
            downloadProgress.value = Math.round((downloaded / total) * 100);
          }
          break;
        case "Finished":
          downloadProgress.value = 100;
          break;
      }
    });
    status.value = "ready";
  } catch (e) {
    status.value = "error";
    errorMsg.value = e instanceof Error ? e.message : String(e);
  }
}

/** 重启应用以完成更新安装 */
async function relaunchApp(): Promise<void> {
  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
}

/** 下载进度通知内容组件 */
const ProgressContent = defineComponent({
  setup() {
    return () =>
      h("div", { style: "display: flex; flex-direction: column; gap: 8px; width: 220px;" }, [
        h("span", { style: "font-size: 12px; color: #6b7280;" }, `${downloadProgress.value}%`),
        h(
          "div",
          {
            style:
              "height: 4px; background: rgba(64, 158, 255, 0.15); border-radius: 2px; overflow: hidden;",
          },
          [
            h("div", {
              style: `height: 100%; background: #409eff; border-radius: 2px; width: ${downloadProgress.value}%; transition: width 0.2s ease;`,
            }),
          ]
        ),
      ]);
  },
});

/** 从通知栏直接启动更新：下载完成确认后重启 */
async function startUpdateFromNotification(): Promise<void> {
  if (status.value === "downloading") {
    return;
  }
  const { ElNotification, ElMessageBox } = await import("element-plus");
  const downloading = downloadAndInstall();
  const progress = ElNotification({
    title: "正在下载更新",
    message: h(ProgressContent),
    type: "info",
    duration: 0,
    position: "bottom-right",
  });
  await downloading;
  progress.close();
  if (status.value === "ready") {
    try {
      await ElMessageBox.confirm(
        `v${updateInfo.value?.version ?? ""} 已下载完成，重启应用以完成安装？`,
        "更新完成",
        { confirmButtonText: "立即重启", cancelButtonText: "稍后", type: "success" }
      );
      await relaunchApp();
    } catch {
      // 用户选择稍后，可在设置页手动重启
    }
  } else if (status.value === "error") {
    ElNotification({
      title: "更新下载失败",
      message: errorMsg.value || "下载更新时出错",
      type: "error",
      duration: 5000,
      position: "bottom-right",
    });
  }
}

/** 已通知过的版本号：同一版本只弹一次通知 */
let notifiedVersion: string | null = null;

/** 弹出「发现新版本」通知 */
function notifyUpdateAvailable(): void {
  if (!updateInfo.value) {
    return;
  }
  const version = updateInfo.value.version;
  if (notifiedVersion === version) {
    return;
  }
  notifiedVersion = version;
  import("element-plus").then(({ ElNotification }) => {
    const notification = ElNotification({
      title: `发现新版本 v${version}`,
      message: h(
        "div",
        { style: "display: flex; align-items: center; gap: 10px;" },
        [
          h("span", { style: "font-size: 12px; color: #6b7280;" }, "点击立即更新"),
          h(
            "button",
            {
              style:
                "border: none; background: #409eff; color: #fff; font-size: 12px; padding: 4px 12px; border-radius: 999px; cursor: pointer;",
              onClick: () => {
                notification.close();
                void startUpdateFromNotification();
              },
            },
            "立即更新"
          ),
        ]
      ),
      type: "info",
      duration: 0,
      position: "bottom-right",
    });
  });
}

/** 应用启动时静默检查更新 */
async function checkOnStartup(): Promise<void> {
  try {
    const hasUpdate = await checkForUpdate(true);
    if (hasUpdate && updateInfo.value) {
      notifyUpdateAvailable();
    }
  } catch {
    // 静默失败
  }
}

/** 周期检查定时器 */
let periodicTimer: ReturnType<typeof setInterval> | null = null;

/** 启动周期性检查（默认 6 小时），重复调用无副作用 */
function startPeriodicCheck(intervalMs = 6 * 60 * 60 * 1000): void {
  if (periodicTimer != null) {
    return;
  }
  periodicTimer = setInterval(() => {
    if (
      status.value === "checking" ||
      status.value === "downloading" ||
      status.value === "ready"
    ) {
      return;
    }
    void checkForUpdate(true).then((has) => {
      if (has) {
        notifyUpdateAvailable();
      }
    });
  }, intervalMs);
}

export function useUpdater() {
  return {
    status: readonly(status),
    updateInfo: readonly(updateInfo),
    errorMsg: readonly(errorMsg),
    downloadProgress: readonly(downloadProgress),
    currentVersion: readonly(currentVersion),
    checkForUpdate,
    downloadAndInstall,
    relaunchApp,
    startUpdateFromNotification,
    checkOnStartup,
    startPeriodicCheck,
  };
}
