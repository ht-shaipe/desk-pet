import { invoke } from "@tauri-apps/api/core";
import type { ChatEntry } from "@/types";

export const getChatMessages = () => invoke<ChatEntry[]>("get_chat_messages");
export const addChatMessage = (role: string, content: string) =>
  invoke<ChatEntry>("add_chat_message", { role, content });
export const deleteChatMessage = (id: number) => invoke<void>("delete_chat_message", { id });
export const clearChatMessages = () => invoke<void>("clear_chat_messages");
