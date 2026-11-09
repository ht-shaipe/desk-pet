import { invoke } from "@tauri-apps/api/core";
import type { ChatEntry } from "@/types";

export const getChatMessages = () => invoke<ChatEntry[]>("get_chat_messages");
/** assistant 工具轮带 toolCalls（ToolCall[]，序列化为 JSON 字符串传给 Rust 存储）；tool 结果消息带 toolCallId。 */
export const addChatMessage = (
  role: string,
  content: string,
  toolCalls?: unknown[],
  toolCallId?: string
) =>
  invoke<ChatEntry>("add_chat_message", {
    role,
    content,
    // Rust 侧 tool_calls 为 Option<String>（JSON 文本），传数组会反序列化失败
    toolCalls: toolCalls ? JSON.stringify(toolCalls) : null,
    toolCallId: toolCallId ?? null,
  });
export const deleteChatMessage = (id: number) => invoke<void>("delete_chat_message", { id });
/** 删除指定 id 及其之后的全部消息（含该条），返回删除条数；用于用户消息「重新执行」。 */
export const deleteChatMessagesFrom = (id: number) =>
  invoke<number>("delete_chat_messages_from", { id });
export const clearChatMessages = () => invoke<void>("clear_chat_messages");
