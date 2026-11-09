/**
 * gRPC 实时通信前端桥接：
 * - 监听 Rust 下发的 hello-ack / task-assign / task-cancel / upgrade / push 事件
 * - hello-ack：握手失败提示一次；pending_tasks 为断线期间补发任务，按 task-assign 同样处理
 * - task_type=1 升级：复用 useUpdater 检查并安装（force 时安装完自动重启）
 * - task_type=4 检查更新：仅检查并提醒用户有新版本，不自动下载安装
 * - task_type=2/3 配置/指令：回执成功（前端可按 payload 自行扩展）
 * - push：Rust 侧统一转发屏幕大字浮层（notify 窗口）；本监听仅作浮层缺失时的角落通知兜底
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { ElNotification } from "element-plus";
import { i18n } from "@/i18n";
import { channelTaskAck, channelTaskProgress } from "@/api/channel";
import { useUpdater } from "@/composables/useUpdater";

/** 任务下发事件载荷（对应 proto TaskAssign）。 */
interface TaskAssignEvent {
  task_id: number;
  task_type: number;
  attempt: number;
  payload: string;
}

/** 握手应答事件载荷（对应 proto HelloAck）。 */
interface HelloAckEvent {
  success: boolean;
  message: string;
  server_time: number;
  pending_tasks: TaskAssignEvent[];
}

/** 直接推送事件载荷（对应 proto PushMessage）。 */
interface PushEvent {
  message_id: string;
  message_type: string;
  payload: string;
  server_time: number;
}

let unlisteners: UnlistenFn[] = [];
/** 升级流程进行中：任务与便捷 Upgrade 消息可能先后到达，只跑一次。 */
let upgrading = false;
/** 握手失败只提醒一次（重连会反复触发 HelloAck）。 */
let helloFailNotified = false;

const updater = useUpdater();

/** 解析任务/升级 payload 中的 force 标记（{version,url,md5,force}）。 */
function parseForce(payload: string): boolean {
  try {
    return JSON.parse(payload)?.force === true;
  } catch {
    return false;
  }
}

/** 升级流程：检查 → 下载安装 → 回执；force 时安装完自动重启。
 *  taskId 为 null 表示便捷 Upgrade 消息，无需回执。
 *  提示不用 pet 区 ElNotification（太小看不到）：非强制升级弹独立置顶更新窗口实时展示进度。 */
async function runUpgradeFlow(taskId: number | null, force: boolean): Promise<void> {
  if (upgrading) {
    await reportProgress(taskId, 4, "merged into concurrent upgrade");
    return;
  }
  upgrading = true;
  try {
    await reportProgress(taskId, 3, "");
    const found = await updater.checkForUpdate(true);
    if (!found) {
      if (updater.status.value === "error") {
        await reportProgress(taskId, 5, updater.errorMsg.value);
        return;
      }
      await reportProgress(taskId, 4, "no update");
      return;
    }
    if (!force) {
      // 独立置顶更新窗实时展示下载进度，完成后用户在窗口里点重启
      void invoke("updater_open").catch(() => undefined);
    }
    await updater.downloadAndInstall();
    if (updater.status.value === "error") {
      await reportProgress(taskId, 5, updater.errorMsg.value);
      return;
    }
    await reportProgress(taskId, 4, updater.updateInfo.value?.version ?? "");
    if (force) {
      await updater.relaunchApp();
    }
  } catch (e) {
    await reportProgress(taskId, 5, String(e));
  } finally {
    upgrading = false;
  }
}

/** 检查更新流程：只检查并提醒（复用 updater 的版本去重/忽略过滤），不自动安装。
 *  taskId 为 null 时无需回执。提示强制走独立置顶更新窗口，不用 pet 区通知。 */
async function runCheckUpdateFlow(taskId: number | null): Promise<void> {
  if (upgrading) {
    await reportProgress(taskId, 4, "merged into concurrent upgrade");
    return;
  }
  await reportProgress(taskId, 3, "");
  const found = await updater.checkForUpdate(true);
  if (!found) {
    if (updater.status.value === "error") {
      await reportProgress(taskId, 5, updater.errorMsg.value);
      return;
    }
    await reportProgress(taskId, 4, "no update");
    return;
  }
  updater.notifyUpdate(true);
  await reportProgress(taskId, 4, updater.updateInfo.value?.version ?? "");
}

/** 任务进度回执（便捷 Upgrade 消息无任务号，跳过）。 */
async function reportProgress(taskId: number | null, status: number, result: string): Promise<void> {
  if (taskId == null) return;
  await channelTaskProgress(taskId, status, result).catch(() => undefined);
}

/** 统一处理任务下发（实时下发与重连补发共用）。 */
async function handleTaskAssign(task: TaskAssignEvent): Promise<void> {
  const { task_id, task_type, attempt, payload } = task;
  await channelTaskAck(task_id, attempt, true, "").catch(() => undefined);
  if (task_type === 1) {
    await runUpgradeFlow(task_id, parseForce(payload));
    return;
  }
  if (task_type === 4) {
    await runCheckUpdateFlow(task_id);
    return;
  }
  await reportProgress(task_id, 4, "");
}

/** 解析推送 payload：优先 JSON {title,content}，否则原样展示。 */
function parsePush(payload: string): { title?: string; content: string } {
  try {
    const obj = JSON.parse(payload);
    if (obj && typeof obj === "object" && (obj.title || obj.content)) {
      return { title: obj.title, content: String(obj.content ?? payload) };
    }
  } catch {
    // 普通文本，原样展示
  }
  return { content: payload };
}

/** 初始化实时通信事件监听（幂等，重复调用不会重复注册）。 */
export async function initChannelListeners(): Promise<void> {
  if (unlisteners.length > 0) return;

  unlisteners.push(
    await listen<HelloAckEvent>("channel://hello-ack", (event) => {
      const { success, message, pending_tasks } = event.payload;
      if (!success) {
        if (!helloFailNotified) {
          helloFailNotified = true;
          ElNotification({
            title: i18n.global.t("channel.helloFailed"),
            message: message || undefined,
            type: "warning",
            duration: 5000,
          });
        }
        return;
      }
      // 重连补发清单：与实时下发同样处理
      for (const task of pending_tasks ?? []) {
        void handleTaskAssign(task);
      }
    }),
  );

  unlisteners.push(
    await listen<TaskAssignEvent>("channel://task-assign", (event) => {
      void handleTaskAssign(event.payload);
    }),
  );

  unlisteners.push(
    await listen<{ task_id: number }>("channel://task-cancel", (event) => {
      console.log("[channel] cancel task", event.payload.task_id);
    }),
  );

  unlisteners.push(
    await listen<{ version: string; url: string; md5: string; force: boolean }>(
      "channel://upgrade",
      (event) => {
        void runUpgradeFlow(null, event.payload.force);
      },
    ),
  );

  unlisteners.push(
    await listen<PushEvent>("channel://push", (event) => {
      const { message_type, payload, server_time } = event.payload;
      const parsed = parsePush(payload);
      const d = new Date((server_time > 0 ? server_time : Date.now() / 1000) * 1000);
      const pad = (n: number) => String(n).padStart(2, "0");
      const full = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
      ElNotification({
        title: `${parsed.title || message_type || i18n.global.t("channel.newMessage")} · ${full}`,
        message: parsed.content,
        type: "info",
        duration: 5000,
      });
    }),
  );
}

/** 清理实时通信事件监听。 */
export function disposeChannelListeners(): void {
  for (const fn of unlisteners) fn();
  unlisteners = [];
  helloFailNotified = false;
}
