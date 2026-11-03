import { invoke } from "@tauri-apps/api/core";

export const channelStart = () => invoke<string>("channel_start");
export const channelStop = () => invoke<void>("channel_stop");
export const channelTaskAck = (
  taskId: number,
  attempt: number,
  success: boolean,
  message: string,
) => invoke<void>("channel_task_ack", { taskId, attempt, success, message });
export const channelTaskProgress = (taskId: number, status: number, result: string) =>
  invoke<void>("channel_task_progress", { taskId, status, result });
