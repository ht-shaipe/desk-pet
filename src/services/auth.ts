import { fetch } from "@tauri-apps/plugin-http";

/** 账户接口基地址（与 stash 扩展同一套服务）。 */
const BASE_API = "https://hub.htui.cc/api/pas/user";

/** 接口信封：成功 code=200，4013=需要人机验证。 */
export interface ApiEnvelope<T> {
  code: number;
  result: T;
  message?: string;
  msg?: string;
}

export type VerifyMode = "slider" | "captcha" | "none";

export interface VerifyConfig {
  login: VerifyMode;
  register: VerifyMode;
}

export interface CaptchaData {
  ticket: string;
  image: string;
}

export interface SliderData {
  ticket: string;
  width: number;
  sliderWidth: number;
  targetX: number;
}

export class ApiError extends Error {
  code: number;

  constructor(code: number, message: string) {
    super(message);
    this.code = code;
  }
}

async function post<T>(path: string, body: Record<string, unknown> = {}): Promise<T> {
  const res = await fetch(`${BASE_API}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  let data: ApiEnvelope<T> | null = null;
  try {
    data = JSON.parse(text) as ApiEnvelope<T>;
  } catch {
    throw new ApiError(-1, `HTTP ${res.status}`);
  }
  if (!data || typeof data.code !== "number") {
    throw new ApiError(-1, "响应格式错误");
  }
  if (data.code !== 200) {
    throw new ApiError(data.code, data.message ?? data.msg ?? `错误码 ${data.code}`);
  }
  return data.result;
}

export const verifyConfig = () => post<VerifyConfig>("/verify/config");

export const getCaptcha = () =>
  post<CaptchaData>("/captcha", { width: 130, height: 40, length: 4 });

export const getSlider = () => post<SliderData>("/slider");

export const mobileExists = (mobile: string) => post<boolean>("/mobile/exists", { mobile });

/** 登录/注册提交参数：ticket + code（图形码）或 x（滑块）。 */
export interface AuthSubmit {
  mobile: string;
  password: string;
  ticket?: string;
  code?: string;
  x?: number;
}

export const login = (payload: AuthSubmit) => post<unknown>("/login", { ...payload });

export const register = (payload: AuthSubmit) => post<unknown>("/register", { ...payload });

/** 从登录/注册返回值中提取 token（可能是字符串或含 token 字段的对象）。 */
export function extractToken(result: unknown): string | null {
  if (typeof result === "string" && result.length > 0) {
    return result;
  }
  if (result && typeof result === "object") {
    const token = (result as Record<string, unknown>)["token"];
    if (typeof token === "string" && token.length > 0) {
      return token;
    }
  }
  return null;
}
