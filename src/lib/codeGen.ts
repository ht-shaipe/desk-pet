/**
 * 请求代码生成：fetch / Python requests（cURL 生成在 curlParser.ts）。
 * 生成的代码以"可直接粘贴运行"为目标：字符串一律用安全字面量，认证预计算进头或参数。
 */
import type { HttpHeader, HttpRequestSpec } from "@/types";

/** 带 query 参数的完整 URL（认证为 query 落点时同样并入） */
function fullUrl(spec: HttpRequestSpec): string {
  const params = [...spec.params];
  if (spec.auth?.type === "apikey" && (spec.auth.in ?? "header") === "query" && spec.auth.key?.trim()) {
    params.push({ name: spec.auth.key.trim(), value: spec.auth.value ?? "" });
  }
  if (!params.length) {
    return spec.url;
  }
  const qs = params
    .filter((p) => p.name.trim())
    .map((p) => `${encodeURIComponent(p.name)}=${encodeURIComponent(p.value)}`)
    .join("&");
  return `${spec.url}${spec.url.includes("?") ? "&" : "?"}${qs}`;
}

/** 认证展开为请求头（无认证落点头时返回 null） */
function authHeader(spec: HttpRequestSpec): HttpHeader | null {
  const auth = spec.auth;
  if (!auth) {
    return null;
  }
  if (auth.type === "bearer" && auth.token?.trim()) {
    return { name: "Authorization", value: `Bearer ${auth.token.trim()}` };
  }
  if (auth.type === "basic" && auth.username) {
    // 用户名密码允许非 Latin1 字符，btoa 前先经 UTF-8 编码
    const bytes = new TextEncoder().encode(`${auth.username}:${auth.password ?? ""}`);
    let bin = "";
    bytes.forEach((b) => {
      bin += String.fromCharCode(b);
    });
    return { name: "Authorization", value: `Basic ${btoa(bin)}` };
  }
  if (auth.type === "apikey" && (auth.in ?? "header") === "header" && auth.key?.trim()) {
    return { name: auth.key.trim(), value: auth.value ?? "" };
  }
  return null;
}

/** 请求头合并认证后的完整列表 */
function allHeaders(spec: HttpRequestSpec): HttpHeader[] {
  const headers = spec.headers.filter((h) => h.name.trim());
  const auth = authHeader(spec);
  if (auth && !headers.some((h) => h.name.toLowerCase() === auth.name.toLowerCase())) {
    headers.push(auth);
  }
  return headers;
}

const hasBody = (spec: HttpRequestSpec): boolean => {
  const t = spec.bodyType ?? "json";
  if (t === "form-urlencoded") {
    return !!spec.form?.some((f) => f.name.trim());
  }
  if (t === "form-data") {
    return !!spec.form?.some((f) => f.name.trim()) || !!spec.formFiles?.some((f) => f.name.trim());
  }
  return !!spec.body;
};

/** 生成 fetch 代码 */
export function toFetch(spec: HttpRequestSpec): string {
  const lines: string[] = [];
  const headers = allHeaders(spec);
  const bodyType = spec.bodyType ?? "json";
  const method = (spec.method || "GET").toUpperCase();

  const headerEntries = headers.map((h) => `    ${JSON.stringify(h.name)}: ${JSON.stringify(h.value)},`);
  let bodyExpr: string | null = null;
  if (bodyType === "form-data") {
    lines.push("const form = new FormData();");
    for (const field of spec.form ?? []) {
      if (field.name.trim()) {
        lines.push(`form.append(${JSON.stringify(field.name)}, ${JSON.stringify(field.value)});`);
      }
    }
    // 文件字段需运行环境提供 Blob/File，这里以注释占位
    for (const file of spec.formFiles ?? []) {
      if (file.name.trim()) {
        lines.push(`// TODO: 将本地文件替换为 File/Blob 后取消注释`);
        lines.push(`// form.append(${JSON.stringify(file.name)}, file, ${JSON.stringify(file.path.split(/[\\/]/).pop() ?? "")});`);
      }
    }
    bodyExpr = "form";
  } else if (bodyType === "form-urlencoded") {
    const pairs = (spec.form ?? []).filter((f) => f.name.trim());
    bodyExpr = `new URLSearchParams({\n${pairs
      .map((f) => `  ${JSON.stringify(f.name)}: ${JSON.stringify(f.value)},`)
      .join("\n")}\n})`;
  } else if (spec.body) {
    let jsonPretty: string | null = null;
    if (bodyType === "json") {
      try {
        jsonPretty = JSON.stringify(JSON.parse(spec.body), null, 2);
      } catch {
        jsonPretty = null;
      }
    }
    // JSON 需经 JSON.stringify 才能作为 fetch body；解析失败按纯文本字符串
    bodyExpr = jsonPretty !== null ? `JSON.stringify(${jsonPretty})` : JSON.stringify(spec.body);
  }

  lines.push(`const res = await fetch(${JSON.stringify(fullUrl(spec))}, {`);
  if (method !== "GET") {
    lines.push(`  method: ${JSON.stringify(method)},`);
  }
  if (headerEntries.length) {
    lines.push("  headers: {");
    lines.push(...headerEntries);
    lines.push("  },");
  }
  if (bodyExpr && hasBody(spec) && method !== "GET" && method !== "HEAD") {
    lines.push(`  body: ${bodyExpr},`);
  }
  if (spec.timeoutMs) {
    lines.push(`  signal: AbortSignal.timeout(${spec.timeoutMs}),`);
  }
  lines.push("});");
  lines.push("console.log(res.status, await res.text());");
  return lines.join("\n");
}

/** JSON.stringify 字面量同时用作 Python 双引号字符串（ASCII/常见转义兼容） */
const pyStr = (v: string): string => JSON.stringify(v);

/** dict 字面量（fetch 用 JS 对象、Python 用 dict，均兼容） */
const dictLiteral = (pairs: HttpHeader[], indent: string): string => {
  const rows = pairs
    .filter((p) => p.name.trim())
    .map((p) => `${indent}    ${pyStr(p.name)}: ${pyStr(p.value)},`);
  return `{\n${rows.join("\n")}\n${indent}}`;
};

/** 生成 Python requests 代码 */
export function toPython(spec: HttpRequestSpec): string {
  const lines: string[] = ["import requests", ""];
  const headers = allHeaders(spec);
  const bodyType = spec.bodyType ?? "json";
  const method = (spec.method || "GET").toLowerCase();
  const args: string[] = [];

  if (spec.params.some((p) => p.name.trim())) {
    args.push(`    params=${dictLiteral(spec.params, "    ")},`);
  }
  if (headers.length) {
    args.push(`    headers=${dictLiteral(headers, "    ")},`);
  }

  if (bodyType === "form-data") {
    const fields = (spec.form ?? []).filter((f) => f.name.trim());
    const files = (spec.formFiles ?? []).filter((f) => f.name.trim());
    if (fields.length) {
      args.push(`    data=${dictLiteral(fields, "    ")},`);
    }
    if (files.length) {
      // TODO 注释紧贴 files 参数，路径需调用方替换
      lines.push("# TODO: 文件字段请将路径替换为实际可访问路径");
      const fileRows = files.map(
        (f) => `    ${pyStr(f.name)}: open(${pyStr(f.path)}, "rb"),`,
      );
      args.push(`    files={\n${fileRows.join("\n")}\n    },`);
    }
  } else if (bodyType === "form-urlencoded") {
    const pairs = (spec.form ?? []).filter((f) => f.name.trim());
    args.push(`    data=${dictLiteral(pairs, "    ")},`);
  } else if (spec.body) {
    let jsonPretty: string | null = null;
    if (bodyType === "json") {
      try {
        jsonPretty = JSON.stringify(JSON.parse(spec.body), null, 2);
      } catch {
        jsonPretty = null;
      }
    }
    if (jsonPretty) {
      // 缩进重排为 4 空格以贴近 Python 风格
      const indented = jsonPretty
        .split("\n")
        .map((row, i) => (i === 0 ? row : `    ${row}`))
        .join("\n");
      args.push(`    json=${indented},`);
    } else {
      args.push(`    data=${pyStr(spec.body)},`);
    }
  }

  if (spec.timeoutMs) {
    args.push(`    timeout=${(spec.timeoutMs / 1000).toFixed(1).replace(/\.0$/, "")},`);
  }

  lines.push(`resp = requests.${method}(`);
  lines.push(`    ${pyStr(fullUrl(spec))},`);
  lines.push(...args);
  lines.push(")");
  lines.push('print(resp.status_code, resp.text)');
  return lines.join("\n");
}
