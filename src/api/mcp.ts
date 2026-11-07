import { invoke } from "@tauri-apps/api/core";
import type { McpToolDto, McpToolResult } from "@/types";

/** 连接 MCP 服务器，返回工具列表（Rust 侧缓存）。 */
export const mcpConnect = (serverId: string) =>
  invoke<McpToolDto[]>("mcp_connect", { serverId });
export const mcpDisconnect = (serverId: string) => invoke<void>("mcp_disconnect", { serverId });
/** 工具列表；未连接且启用时 Rust 侧自动懒连接。 */
export const mcpServerTools = (serverId: string) =>
  invoke<McpToolDto[]>("mcp_server_tools", { serverId });
/** 调用工具；argsJson 为 JSON 对象字符串，返回聚合文本。 */
export const mcpCallTool = (serverId: string, tool: string, argsJson: string) =>
  invoke<McpToolResult>("mcp_call_tool", { serverId, tool, arguments: argsJson });
/** 已连接的服务器 id 列表。 */
export const mcpServerStatus = () => invoke<string[]>("mcp_server_status");
