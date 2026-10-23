import { fetch } from "@tauri-apps/plugin-http";
import type { ClientOptions } from "@tauri-apps/plugin-http";
import type { AiModel, ChatMessage } from "@/types";

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
    delta?: { content?: string | null };
  }>;
}

/** 从 SSE 行中提取增量文本；返回 null 表示结束（[DONE]）。 */
function extractDelta(line: string): string | null | undefined {
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
    const choice = chunk.choices?.[0];
    return choice?.delta?.content ?? undefined;
  } catch {
    return undefined;
  }
}

export interface StreamChatOptions {
  model: AiModel;
  messages: ChatMessage[];
  signal?: AbortSignal;
  onDelta: (text: string) => void;
}

/**
 * 调用 OpenAI 兼容接口流式输出回复。
 * 服务端不支持流式时回退为一次性返回完整文本。
 */
export async function streamChatCompletion(options: StreamChatOptions): Promise<void> {
  const { model, messages, signal, onDelta } = options;
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
      messages: messages.map((m) => ({ role: m.role, content: m.content })),
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
        choices?: Array<{ message?: { content?: string } }>;
      };
      const content = data.choices?.[0]?.message?.content ?? "";
      if (content) {
        onDelta(content);
      }
      return;
    } catch {
      throw new AiApiError(0, "Invalid response JSON");
    }
  }
  const reader = body.getReader();
  const decoder = new TextDecoder("utf-8");
  let buffer = "";
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
      const delta = extractDelta(line);
      if (delta === null) {
        return;
      }
      if (typeof delta === "string" && delta.length > 0) {
        onDelta(delta);
      }
    }
  }
}

/** 校验模型配置是否具备调用条件。 */
export function isModelReady(model: AiModel): boolean {
  return (
    model.base_url.trim().length > 0 &&
    model.api_key.trim().length > 0 &&
    model.model.trim().length > 0
  );
}
