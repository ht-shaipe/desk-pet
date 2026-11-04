import { invoke } from "@tauri-apps/api/core";

// token 由前端直传（空串 = 匿名连接），避免依赖 Rust 侧磁盘读取与前端防抖落盘的时序
export const channelStart = (token: string) => invoke<string>("channel_start", { token });
export const channelStop = () => invoke<void>("channel_stop");
export const channelTaskAck = (
  taskId: number,
  attempt: number,
  success: boolean,
  message: string,
) => invoke<void>("channel_task_ack", { taskId, attempt, success, message });
export const channelTaskProgress = (taskId: number, status: number, result: string) =>
  invoke<void>("channel_task_progress", { taskId, status, result });
