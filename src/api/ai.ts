import { fetch } from "@tauri-apps/plugin-http";
import type { ClientOptions } from "@tauri-apps/plugin-http";
import type { AiModel, ChatMessage, ToolCall, ToolDefinition } from "@/types";

/** 拼接 base_url 与路径，容忍末尾斜杠缺失。 */
function joinUrl(base: string, path: string): string {
  const b = base.replace(/\/+$/, "");
  return `${b}${path}`;
}

/**
 * 是否为本机或内网地址：证书回退仅对这些地址生效，公网地址保持严格校验。
 * （本机 https 服务多为自签名证书，系统 CA 链验证不通过。）
 */
function isLocalOrPrivateHost(hostname: string): boolean {
  const h = hostname.toLowerCase().replace(/^\[|\]$/g, "");
  if (h === "localhost" || h === "::1") {
    return true;
  }
  const m = h.match(/^(\d{1,3})\.(\d{1,3})(?:\.\d{1,3}){2}$/);
  if (!m) {
    return false;
  }
  const a = Number(m[1]);
  const b = Number(m[2]);
  return (
    a === 10 || a === 127 || (a === 192 && b === 168) || (a === 172 && b >= 16 && b <= 31)
  );
}

/**
 * 带证书回退的请求：严格校验失败（多为本地服务自签名证书）且目标为本机/内网时，
 * 自动忽略证书校验重试一次。
 */
async function fetchWithTlsFallback(url: string, init: RequestInit & ClientOptions): Promise<Response> {
  let parsed: URL | null = null;
  try {
    parsed = new URL(url);
  } catch {
    // 无效 URL 交由下方正常请求抛出
  }
  if (!parsed || parsed.protocol !== "https:" || !isLocalOrPrivateHost(parsed.hostname)) {
    return fetch(url, init);
  }
  try {
    return await fetch(url, init);
  } catch (err) {
    if (!String(err).includes("error sending request")) {
      throw err;
    }
    console.warn("[desk-pet] TLS 验证失败，忽略证书重试:", url);
    return fetch(url, {
      ...init,
      danger: { acceptInvalidCerts: true, acceptInvalidHostnames: true },
    });
  }
}

export class AiApiError extends Error {
  status: number;

  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

/** OpenAI 兼容 /chat/completions 的流式响应分块。 */
interface ChatCompletionChunk {
  choices?: Array<{
    delta?: {
      content?: string | null;
      /** 工具调用增量：按 index 分片拼接 id/name/arguments */
      tool_calls?: Array<{
        index?: number;
        id?: string;
        type?: string;
        function?: { name?: string; arguments?: string };
      }>;
    };
  }>;
}

/** 流式工具调用累积器（OpenAI 分片：首片带 id/name，后续片只有 arguments 增量）。 */
class ToolCallAccumulator {
  private parts = new Map<number, { id: string; name: string; arguments: string }>();

  push(fragments: NonNullable<NonNullable<ChatCompletionChunk["choices"]>[number]["delta"]>["tool_calls"]): void {
    if (!fragments) {
      return;
    }
    for (const f of fragments) {
      const idx = typeof f.index === "number" ? f.index : this.parts.size;
      const part = this.parts.get(idx) ?? { id: "", name: "", arguments: "" };
      if (f.id) {
        part.id = f.id;
      }
      if (f.function?.name) {
        part.name = f.function.name;
      }
      if (f.function?.arguments) {
        part.arguments += f.function.arguments;
      }
      this.parts.set(idx, part);
    }
  }

  /** 汇总为完整 ToolCall 列表（按 index 顺序；缺 id 时按序补齐）。 */
  finish(): ToolCall[] {
    return [...this.parts.entries()]
      .sort(([a], [b]) => a - b)
      .map(([, p], i) => ({
        id: p.id || `call_${Date.now().toString(36)}_${i}`,
        type: "function",
        function: { name: p.name, arguments: p.arguments },
      }));
  }

  get size(): number {
    return this.parts.size;
  }
}

/** 从 SSE 行中提取增量：null 表示结束（[DONE]），undefined 表示行内无有效内容。 */
function extractDelta(
  line: string,
  acc: ToolCallAccumulator
): { text?: string } | null | undefined {
  if (line === "data: [DONE]") {
    return null;
  }
  if (!line.startsWith("data:")) {
    return undefined;
  }
  const payload = line.slice(5).trim();
  if (!payload) {
    return undefined;
  }
  try {
    const chunk = JSON.parse(payload) as ChatCompletionChunk;
    const delta = chunk.choices?.[0]?.delta;
    if (delta?.tool_calls) {
      acc.push(delta.tool_calls);
    }
    const content = delta?.content;
    return typeof content === "string" && content.length > 0 ? { text: content } : undefined;
  } catch {
    return undefined;
  }
}

export interface StreamChatOptions {
  model: AiModel;
  messages: ChatMessage[];
  /** 可选工具列表：传入后模型可返回 tool_calls（OpenAI function-calling 格式） */
  tools?: ToolDefinition[];
  signal?: AbortSignal;
  onDelta: (text: string) => void;
}

/** 一轮流式调用的结果：文本 + 工具调用（二者至少其一）。 */
export interface StreamChatResult {
  content: string;
  toolCalls: ToolCall[];
}

/**
 * 调用 OpenAI 兼容接口流式输出回复。
 * 服务端不支持流式时回退为一次性返回完整文本；支持 tools 时返回模型请求的工具调用。
 */
export async function streamChatCompletion(options: StreamChatOptions): Promise<StreamChatResult> {
  const { model, messages, tools, signal, onDelta } = options;
  const url = joinUrl(model.base_url, "/chat/completions");
  const res = await fetchWithTlsFallback(url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${model.api_key}`,
      "User-Agent": "DeskPet",
    },
    body: JSON.stringify({
      model: model.model,
      messages: messages.map((m) => ({
        role: m.role,
        content: m.content,
        ...(m.tool_calls ? { tool_calls: m.tool_calls } : {}),
        ...(m.tool_call_id ? { tool_call_id: m.tool_call_id } : {}),
      })),
      ...(tools && tools.length > 0 ? { tools } : {}),
      stream: true,
    }),
    signal,
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new AiApiError(res.status, text || `HTTP ${res.status}`);
  }
  const body = res.body;
  if (!body) {
    throw new AiApiError(0, "Empty response body");
  }
  // 服务端忽略 stream 参数时返回普通 JSON，一次性取全文
  const contentType = res.headers.get("content-type") ?? "";
  if (!contentType.includes("text/event-stream")) {
    const text = await res.text();
    try {
      const data = JSON.parse(text) as {
        choices?: Array<{
          message?: { content?: string | null; tool_calls?: ToolCall[] };
        }>;
      };
      const message = data.choices?.[0]?.message;
      const content = message?.content ?? "";
      if (content) {
        onDelta(content);
      }
      return { content, toolCalls: message?.tool_calls ?? [] };
    } catch {
      throw new AiApiError(0, "Invalid response JSON");
    }
  }
  const acc = new ToolCallAccumulator();
  const reader = body.getReader();
  const decoder = new TextDecoder("utf-8");
  let buffer = "";
  let content = "";
  for (;;) {
    const { done, value } = await reader.read();
    if (done) {
      break;
    }
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split("\n");
    buffer = lines.pop() ?? "";
    for (const rawLine of lines) {
      const line = rawLine.trim();
      if (!line) {
        continue;
      }
      const delta = extractDelta(line, acc);
      if (delta === null) {
        return { content, toolCalls: acc.finish() };
      }
      if (delta?.text) {
        content += delta.text;
        onDelta(delta.text);
      }
    }
  }
  return { content, toolCalls: acc.finish() };
}

/** 校验模型配置是否具备调用条件。 */
export function isModelReady(model: AiModel): boolean {
  return (
    model.base_url.trim().length > 0 &&
    model.api_key.trim().length > 0 &&
    model.model.trim().length > 0
  );
}
