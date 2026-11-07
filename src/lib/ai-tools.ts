/**
 * AI 对话的可调用工具注册表（MCP 形态）。
 *
 * - 内置工具（list_apps / open_app / open_tool）：直接在前端执行，无需确认；
 * - MCP 工具（真实 MCP 服务器）：经 Rust rmcp 客户端调用，执行前弹确认框。
 *
 * collectTools() 把两者聚合成 OpenAI function-calling 的 tools 数组，
 * 并给出 函数名 → 执行器 的映射，供 Chat 的 agent 循环路由。
 */
import { h } from "vue";
import { ElMessageBox } from "element-plus";
import { i18n } from "@/i18n";
import { listApps, launchItem } from "@/api/launcher";
import { mcpCallTool, mcpServerTools } from "@/api/mcp";
import { TOOLBOX_TOOLS } from "@/components/toolbox/registry";
import type { McpServerConfig, ToolDefinition } from "@/types";
import { useUiStore, type PanelView } from "@/stores/ui";
import { useConfigStore } from "@/stores/config";
import { usePomodoroStore, type PomoMode } from "@/stores/pomodoro";

/** 工具执行结果：text 会作为 role:"tool" 消息回填给模型。 */
export interface ToolExecResult {
  ok: boolean;
  text: string;
}

/** 一个可被 AI 调用的工具（内置实现）。 */
export interface AiTool {
  name: string;
  description: string;
  /** JSON Schema */
  parameters: Record<string, unknown>;
  execute(args: Record<string, unknown>): Promise<ToolExecResult>;
}

/** MCP 工具的路由信息：执行时转发给对应服务器。 */
interface McpRoute {
  serverId: string;
  toolName: string;
}

const t = (key: string, params?: Record<string, unknown>) => i18n.global.t(key, params ?? {});

/** agent 循环的 system prompt（模型侧约定，非 UI 文案，双语内联）。 */
export const SYSTEM_PROMPT = [
  "你是 Desk Pet 桌面宠物的内置助手 You are the built-in assistant of the Desk Pet app.",
  "你可以调用工具完成操作 You can call tools to perform actions:",
  "- open_app: 打开/启动用户本地的应用 Launch a locally installed app. 支持中英文别名与模糊匹配（飞书/Lark、微信/WeChat 等）Supports CN/EN aliases & fuzzy matching — 直接按用户的说法调用，不要先查询应用列表 call it directly with the user's wording, do NOT list apps first; 未命中时返回候选列表 returns candidates on miss.",
  "- list_apps: 列出本机已安装应用 List installed apps. 仅当用户询问装了什么应用时才使用 Only when the user asks what is installed.",
  "- open_pomodoro: 打开并启动应用内番茄钟 Open and start the in-app Pomodoro timer. 用户提及 计时/专注/番茄钟/休息（长休息/短休息）等意图时调用 Call whenever the user mentions timing/focus/pomodoro/break intent (mode: focus=专注/short=短休息/long=长休息, action: start/pause/reset/open).",
  "- open_view: 切换到应用内的界面 Switch to an in-app view（快速启动/番茄钟/剪贴板/收藏/工具箱/账户/设置等）. 用户提及本应用已有功能或界面时直接调用切换，而不是只给文字指引 Call it when the user mentions an in-app feature/view instead of giving text-only guidance.",
  "- open_tool: 打开应用内工具（工具箱）Open an in-app toolbox tool (qr/base64/des/imgcompress/http/cleanup).",
  "- 其他 MCP 工具按其描述使用 Use other MCP tools per their descriptions.",
  "用户要求打开某应用或工具时优先直接调用对应工具，而不是只给出文字指引；",
  "调用后用一句话简要报告结果。没有合适工具或只是闲聊/问答时直接回答，不要强行调用工具。",
  "Always reply in the user's language, keep it concise.",
].join("\n");

/** ---------- 内置工具 ---------- */

/** 应用名归一化：小写并去除空白，用于匹配比较。 */
function normAppName(name: string): string {
  return name.toLowerCase().replace(/\s+/g, "");
}

/**
 * 常见应用别名组：组内任一名称可互相命中（飞书 ↔ Lark、微信 ↔ WeChat 等），
 * 解决"输入中文名但本机应用是英文名"的匹配缺口。条目统一小写。
 */
const APP_ALIAS_GROUPS: string[][] = [
  ["飞书", "feishu", "lark"],
  ["微信", "wechat", "weixin"],
  ["企业微信", "wxwork", "wecom", "wechatwork"],
  ["钉钉", "dingtalk"],
  ["qq音乐", "qqmusic"],
  ["qq浏览器", "qqbrowser"],
  ["腾讯会议", "voovmeeting", "tencentmeeting"],
  ["网易云音乐", "neteasemusic", "neteasecloudmusic"],
  ["谷歌浏览器", "googlechrome", "chrome"],
  ["wps", "wpsoffice"],
  ["vscode", "visualstudiocode", "code"],
  ["访达", "finder"],
  ["备忘录", "notes"],
  ["提醒事项", "reminders"],
  ["日历", "calendar"],
  ["邮件", "mail"],
  ["地图", "maps"],
  ["计算器", "calculator"],
  ["词典", "dictionary"],
  ["设置", "系统设置", "systemsettings"],
  ["终端", "terminal"],
  ["音乐", "music"],
  ["照片", "photos"],
  ["播客", "podcasts"],
  ["快捷指令", "shortcuts"],
  ["百度网盘", "baidunetdisk"],
  ["迅雷", "thunder"],
  ["印象笔记", "evernote"],
  ["有道词典", "youdao"],
  ["向日葵", "sunlogin"],
];

/** 查询词 → 归一化后的本身 + 所在别名组其余成员。 */
function expandAppAliases(query: string): string[] {
  const q = normAppName(query);
  const terms = [q];
  for (const group of APP_ALIAS_GROUPS) {
    if (group.includes(q)) {
      terms.push(...group.filter((alias) => alias !== q));
    }
  }
  return terms;
}

const BUILTIN_TOOLS: AiTool[] = [
  {
    name: "list_apps",
    description:
      "列出本机已安装的应用 List installed applications on this machine. 无必填参数 No required parameters.",
    parameters: { type: "object", properties: {}, additionalProperties: false },
    execute: async () => {
      const apps = await listApps();
      const MAX = 200;
      const trimmed = apps.slice(0, MAX).map((a) => ({ name: a.name }));
      const note = apps.length > MAX ? ` (showing ${MAX}/${apps.length})` : "";
      return { ok: true, text: JSON.stringify({ apps: trimmed }) + note };
    },
  },
  {
    name: "open_app",
    description:
      "按名称打开/启动本机应用 Open or launch a local app by name. 参数 parameters: {name: 应用名 app name (string, required)}。常见应用支持中英文别名 Common apps accept CN/EN aliases (e.g. 飞书/Feishu/Lark, 微信/WeChat).",
    parameters: {
      type: "object",
      properties: { name: { type: "string", description: "应用名 App name" } },
      required: ["name"],
      additionalProperties: false,
    },
    execute: async (args) => {
      const query = String(args.name ?? "").trim();
      if (!query) {
        return { ok: false, text: "缺少应用名 Missing app name" };
      }
      const apps = await listApps();
      const terms = expandAppAliases(query);
      const hit =
        apps.find((a) => terms.some((q) => normAppName(a.name) === q)) ??
        apps.find((a) =>
          terms.some((q) => {
            const name = normAppName(a.name);
            return name.includes(q) || q.includes(name);
          })
        );
      if (!hit) {
        const candidates = apps
          .slice(0, 30)
          .map((a) => a.name)
          .join(", ");
        return {
          ok: false,
          text: `未找到应用 "${query}"。App not found. 已安装的应用示例 Candidates: ${candidates}`,
        };
      }
      await launchItem({
        id: "ai-open-app",
        name: hit.name,
        icon: "",
        item_type: "app",
        value: hit.value,
        color: "",
      });
      return { ok: true, text: `已启动应用 Launched app: ${hit.name}` };
    },
  },
  {
    name: "open_pomodoro",
    description:
      "打开/控制应用内番茄钟 Open or control the in-app Pomodoro timer. 用户提及 计时、专注、番茄钟、休息（长/短休息）等意图时调用 Call when the user mentions timing/focus/pomodoro/break intent.",
    parameters: {
      type: "object",
      properties: {
        mode: {
          type: "string",
          enum: ["focus", "short", "long"],
          description: "模式 Mode: focus=专注计时 short=短休息 long=长休息（默认 focus）",
        },
        action: {
          type: "string",
          enum: ["start", "pause", "reset", "open"],
          description: "动作 Action（默认 start）: start=启动 pause=暂停 reset=重置 open=仅打开不计时",
        },
      },
      additionalProperties: false,
    },
    execute: async (args) => {
      const MODES = ["focus", "short", "long"] as const;
      const ACTIONS = ["start", "pause", "reset", "open"] as const;
      const mode: PomoMode = MODES.includes(args.mode as (typeof MODES)[number])
        ? (args.mode as PomoMode)
        : "focus";
      const action = ACTIONS.includes(args.action as (typeof ACTIONS)[number])
        ? (args.action as (typeof ACTIONS)[number])
        : "start";
      const pomo = usePomodoroStore();
      // 展开面板 -> 切到番茄钟视图
      useUiStore().openView("pomodoro");
      if (pomo.mode !== mode) {
        pomo.setMode(mode);
      }
      if (action === "pause") {
        pomo.pause();
      } else if (action === "reset") {
        pomo.reset();
      } else if (action === "start" && !pomo.running) {
        pomo.start();
      }
      const modeText = { focus: "专注", short: "短休息", long: "长休息" }[mode];
      const actionText =
        action === "pause"
          ? `已暂停 Paused ${modeText}`
          : action === "reset"
            ? `已重置 Reset ${modeText}`
            : action === "open"
              ? `已打开番茄钟 Pomodoro opened (${modeText})`
              : `已开始${modeText}计时 Started ${mode} timer: ${pomo.display}`;
      return { ok: true, text: actionText };
    },
  },
  {
    name: "open_view",
    description:
      "切换到应用内界面 Switch to an in-app view. 用户提及本应用已有功能/界面时直接调用 Call when the user mentions an in-app feature/view.",
    parameters: {
      type: "object",
      properties: {
        view: {
          type: "string",
          enum: ["launcher", "pomodoro", "clipboard", "stash", "toolbox", "chat", "account", "settings"],
          description:
            "目标界面 Target view: launcher=快速启动 pomodoro=番茄钟 clipboard=剪贴板 stash=收藏/便签 toolbox=工具箱 chat=AI助手 account=账户 settings=设置",
        },
      },
      required: ["view"],
      additionalProperties: false,
    },
    execute: async (args) => {
      const VIEW_NAMES: Record<string, string> = {
        launcher: "快速启动",
        pomodoro: "番茄钟",
        clipboard: "剪贴板",
        stash: "收藏",
        toolbox: "工具箱",
        chat: "AI 助手",
        account: "账户",
        settings: "设置",
      };
      const view = String(args.view ?? "");
      const name = VIEW_NAMES[view];
      if (!name) {
        return {
          ok: false,
          text: `未知界面 Unknown view: "${view}"。可用 Available: ${Object.keys(VIEW_NAMES).join(", ")}`,
        };
      }
      useUiStore().openView(view as PanelView);
      return { ok: true, text: `已切换到「${name}」Switched to ${name}` };
    },
  },
  {
    name: "open_tool",
    description: `打开应用内工具箱的工具 Open an in-app toolbox tool. 可用工具 Available tools: ${TOOLBOX_TOOLS.map(
      (tool) => `${tool.id}=${t(tool.titleKey)}`
    ).join(", ")}`,
    parameters: {
      type: "object",
      properties: {
        tool_id: {
          type: "string",
          enum: TOOLBOX_TOOLS.map((tool) => tool.id),
          description: "工具 id Tool id",
        },
      },
      required: ["tool_id"],
      additionalProperties: false,
    },
    execute: async (args) => {
      const id = String(args.tool_id ?? "").trim();
      const tool = TOOLBOX_TOOLS.find((item) => item.id === id);
      if (!tool) {
        return {
          ok: false,
          text: `未知工具 Unknown tool: "${id}"。可用 Available: ${TOOLBOX_TOOLS.map((item) => item.id).join(", ")}`,
        };
      }
      useUiStore().openToolboxTool(tool.id);
      return { ok: true, text: `已打开工具 Opened tool: ${t(tool.titleKey)}` };
    },
  },
];

/** ---------- MCP 工具聚合 ---------- */

/** 服务器名净化为 OpenAI 函数名允许的字符（^[a-zA-Z0-9_-]{1,64}$）。 */
function sanitizeName(name: string): string {
  const cleaned = name.replace(/[^a-zA-Z0-9_-]/g, "_");
  return cleaned.length > 0 ? cleaned : "srv";
}

/** 组装 MCP 工具的对外函数名 mcp__<server>__<tool>，并处理超长与重名。 */
function mcpFunctionName(server: McpServerConfig, toolName: string, used: Set<string>): string {
  const base = `mcp__${sanitizeName(server.name)}__`;
  const safeTool = toolName.replace(/[^a-zA-Z0-9_-]/g, "_");
  let name = (base + safeTool).slice(0, 64);
  if (used.has(name)) {
    // 同名冲突（服务器重名或截断后撞名）：用配置 id 后缀区分
    const suffix = `_${server.id.slice(-4)}`;
    name = (base + safeTool).slice(0, 64 - suffix.length) + suffix;
  }
  used.add(name);
  return name;
}

export interface AggregatedTools {
  definitions: ToolDefinition[];
  /** 函数名 → 执行器（内置直接执行；MCP 先确认再转发 Rust） */
  executors: Map<string, (args: Record<string, unknown>) => Promise<ToolExecResult>>;
  /** 本次聚合中实际可用的 MCP 服务器数 */
  mcpServerCount: number;
}

/**
 * 聚合内置工具与所有已启用 MCP 服务器的工具。
 * 单个 MCP 服务器连接失败不影响其余工具（跳过并记录日志）。
 */
export async function collectTools(): Promise<AggregatedTools> {
  const definitions: ToolDefinition[] = [];
  const executors = new Map<string, (args: Record<string, unknown>) => Promise<ToolExecResult>>();
  const usedNames = new Set<string>();

  for (const tool of BUILTIN_TOOLS) {
    definitions.push({
      type: "function",
      function: { name: tool.name, description: tool.description, parameters: tool.parameters },
    });
    executors.set(tool.name, (args) => tool.execute(args));
  }

  const servers = useConfigStore().settings.mcp_servers ?? [];
  let mcpServerCount = 0;
  for (const server of servers) {
    if (server.enabled === false) {
      continue;
    }
    try {
      const tools = await mcpServerTools(server.id);
      mcpServerCount += 1;
      for (const tool of tools) {
        const fnName = mcpFunctionName(server, tool.name, usedNames);
        const route: McpRoute = { serverId: server.id, toolName: tool.name };
        definitions.push({
          type: "function",
          function: {
            name: fnName,
            description:
              tool.description ||
              tool.title ||
              `MCP tool ${tool.name} from server ${server.name}`,
            parameters:
              (tool.input_schema as ToolDefinition["function"]["parameters"]) ?? {
                type: "object",
                properties: {},
              },
          },
        });
        executors.set(fnName, (args) => execMcpTool(server.name, tool.name, route, args));
      }
    } catch (err) {
      console.error(`[desk-pet] MCP 服务器 ${server.name} 工具获取失败:`, err);
    }
  }
  return { definitions, executors, mcpServerCount };
}

/** 执行 MCP 工具：先弹确认框（外部工具可能产生任意副作用），拒绝则回填拒绝结果。 */
async function execMcpTool(
  serverName: string,
  toolName: string,
  route: McpRoute,
  args: Record<string, unknown>
): Promise<ToolExecResult> {
  const argsSummary = JSON.stringify(args, null, 2);
  try {
    await ElMessageBox.confirm(
      h("div", { class: "text-[12.5px] leading-[1.6]" }, [
        h("div", null, t("chat.mcpConfirmDesc", { tool: toolName })),
        h(
          "pre",
          {
            class:
              "m-0 mt-1.5 max-h-[150px] overflow-auto whitespace-pre-wrap break-all rounded-lg bg-dp-btn-bg p-2 text-[11px] leading-[1.5] text-dp-tertiary",
          },
          argsSummary.slice(0, 600)
        ),
      ]),
      t("chat.mcpConfirmTitle", { server: serverName }),
      {
        confirmButtonText: t("common.confirm"),
        cancelButtonText: t("common.cancel"),
        type: "warning",
      }
    );
  } catch {
    return { ok: false, text: "用户拒绝了此次工具调用 User denied this tool call." };
  }
  try {
    const result = await mcpCallTool(route.serverId, route.toolName, JSON.stringify(args));
    return { ok: result.ok, text: result.text || t("chat.mcpEmptyResult") };
  } catch (err) {
    return { ok: false, text: `工具执行失败 Tool call failed: ${String(err)}` };
  }
}
