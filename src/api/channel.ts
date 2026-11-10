import { invoke } from "@tauri-apps/api/core";

/** 登录态用户资料（随 Hello 与补报帧上报，服务端据此绑定设备归属）。 */
export interface ChannelUserReport {
  /** 用户唯一标识（业务侧账号体系） */
  userId: string;
  /** 登录账号 */
  username?: string;
  /** 昵称/展示名 */
  nickname?: string;
  /** 手机号 */
  mobile?: string;
  /** 头像 URL */
  avatar?: string;
}

// token 由前端直传（空串 = 匿名连接），避免依赖 Rust 侧磁盘读取与前端防抖落盘的时序；
// user 未登录传 null，已登录时携带用户资料随 Hello 首帧上报。
export const channelStart = (token: string, user?: ChannelUserReport | null) =>
  invoke<string>("channel_start", { token, user: user ?? null });
// 连接保持期间的用户资料补报（user 传 null = 显式解绑，登出时调用）
export const channelReportUser = (user: ChannelUserReport | null) =>
  invoke<void>("channel_report_user", { user });
export const channelStop = () => invoke<void>("channel_stop");
export const channelTaskAck = (
  taskId: number,
  attempt: number,
  success: boolean,
  message: string,
) => invoke<void>("channel_task_ack", { taskId, attempt, success, message });
export const channelTaskProgress = (taskId: number, status: number, result: string) =>
  invoke<void>("channel_task_progress", { taskId, status, result });
