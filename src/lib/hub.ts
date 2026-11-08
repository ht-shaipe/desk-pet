/**
 * hub.htui.cc 接口信封：所有接口统一返回 {code, result, message}，
 * 成功 code=200，业务数据在 result 里（auth.ts 的 ApiEnvelope 同构）。
 */
export interface HubEnvelope<T> {
  code: number;
  result: T;
  message?: string;
  msg?: string;
}

/** 解包 hub 信封响应文本：非 200 code 或 result 缺失时抛错，成功返回 result。 */
export function unwrapHub<T>(raw: string): T {
  const data = JSON.parse(raw) as HubEnvelope<T>;
  if (!data || typeof data.code !== "number") {
    throw new Error(`响应格式错误: ${raw.slice(0, 200)}`);
  }
  if (data.code !== 200) {
    const msg = data.message ?? data.msg ?? "";
    throw new Error(`code ${data.code}${msg ? ` - ${msg}` : ""}`);
  }
  if (data.result == null) {
    throw new Error(`响应 result 为空: ${raw.slice(0, 200)}`);
  }
  return data.result;
}
