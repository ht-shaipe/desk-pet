/**
 * cURL 命令解析与生成（纯函数，无 UI 依赖）。
 *
 * 解析覆盖常用 flag：-X/--request、-H/--header、-d/--data/--data-raw/--data-binary、
 * --data-urlencode、-F/--form（含 @file）、-u/--user、-b/--cookie、-G/--get；
 * 其余 flag（-L/-k/-s/--compressed 等）忽略。词法支持单双引号、反斜杠转义与行续接。
 */
import type { HttpAuth, HttpHeader, HttpFormFile, HttpRequestSpec } from "@/types";

/** shell 风格分词：引号包裹保留原文，反斜杠转义，行续接（\ + 换行）拼接。 */
function tokenize(input: string): string[] {
  const tokens: string[] = [];
  let cur = "";
  let hasCur = false;
  let i = 0;
  const flush = () => {
    if (hasCur) {
      tokens.push(cur);
      cur = "";
      hasCur = false;
    }
  };
  while (i < input.length) {
    const ch = input[i];
    if (ch === "\\") {
      // 行尾续接：直接吞掉换行；其余转义保留下一字符
      const next = input[i + 1];
      if (next === "\n") {
        i += 2;
        continue;
      }
      if (next === "\r" && input[i + 2] === "\n") {
        i += 3;
        continue;
      }
      if (next !== undefined) {
        cur += next;
        hasCur = true;
        i += 2;
        continue;
      }
      i += 1;
      continue;
    }
    if (ch === "'") {
      const end = input.indexOf("'", i + 1);
      const stop = end === -1 ? input.length : end;
      cur += input.slice(i + 1, stop);
      hasCur = true;
      i = stop + 1;
      continue;
    }
    if (ch === '"') {
      hasCur = true;
      i += 1;
      while (i < input.length && input[i] !== '"') {
        if (input[i] === "\\" && i + 1 < input.length) {
          const escaped = input[i + 1];
          // 双引号内仅 \" \\ \$ \` 与换行续接有特殊含义，其余保留反斜杠
          if ('"\\$`'.includes(escaped) || escaped === "\n") {
            if (escaped !== "\n") {
              cur += escaped;
            }
            i += 2;
            continue;
          }
        }
        cur += input[i];
        i += 1;
      }
      i += 1;
      continue;
    }
    if (/\s/.test(ch)) {
      flush();
      i += 1;
      continue;
    }
    if (ch === "#" && !hasCur) {
      // 词首 # 为注释，吞到行尾
      const nl = input.indexOf("\n", i);
      i = nl === -1 ? input.length : nl;
      continue;
    }
    cur += ch;
    hasCur = true;
    i += 1;
  }
  flush();
  return tokens;
}

/** 单引号 shell 转义：'...' 内的 ' 写成 '\'' */
function shQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

/** 同名头覆盖旧值（curl 语义上多次 -H 由服务器决定合并方式，这里保持唯一行） */
function setHeader(headers: HttpHeader[], name: string, value: string) {
  const found = headers.find((h) => h.name.toLowerCase() === name.toLowerCase());
  if (found) {
    found.value = value;
  } else {
    headers.push({ name, value });
  }
}

/** URL 的查询串提取为参数行，返回去掉 search 的 URL 文本。
    无查询串且无参数时原样返回，避免 URL 规范化改变用户输入（如补尾斜杠）。 */
function extractQueryParams(rawUrl: string, params: HttpHeader[]): string {
  if (!params.length && !rawUrl.includes("?")) {
    return rawUrl;
  }
  try {
    const parsed = new URL(rawUrl);
    parsed.searchParams.forEach((value, name) => {
      params.push({ name, value });
    });
    parsed.search = "";
    parsed.hash = "";
    return parsed.toString();
  } catch {
    return rawUrl;
  }
}

/**
 * 解析 cURL 文本；无法识别整体结构（无 URL）时抛错。
 * 解析结果未覆盖的字段保持默认（auth none、params 来自 URL 查询串）。
 */
export function parseCurl(text: string): HttpRequestSpec {
  const tokens = tokenize(text.trim());
  if (tokens[0]?.toLowerCase().endsWith("curl")) {
    tokens.shift();
  }
  if (!tokens.length) {
    throw new Error("empty");
  }

  const spec: HttpRequestSpec = {
    url: "",
    method: "",
    params: [],
    headers: [],
  };
  const form: HttpHeader[] = [];
  const formFiles: HttpFormFile[] = [];
  const dataParts: string[] = [];
  const dataUrlEncoded: HttpHeader[] = [];
  let auth: HttpAuth | null = null;
  let moveToQuery = false; // -G：data 并入查询串
  let sawForm = false;

  const take = (): string => {
    const value = tokens.shift();
    if (value === undefined) {
      throw new Error("missing-arg");
    }
    return value;
  };

  while (tokens.length) {
    const token = take();
    const flag = token.toLowerCase();
    if (!flag.startsWith("-")) {
      // 首个裸词为 URL
      if (!spec.url) {
        spec.url = token;
      }
      continue;
    }
    switch (flag) {
      case "-x":
      case "--request":
        spec.method = take().toUpperCase();
        break;
      case "--url":
        spec.url = take();
        break;
      case "-h":
      case "--header": {
        const raw = take();
        const idx = raw.indexOf(":");
        if (idx > 0) {
          setHeader(spec.headers, raw.slice(0, idx).trim(), raw.slice(idx + 1).trim());
        }
        break;
      }
      case "-d":
      case "--data":
      case "--data-raw":
      case "--data-binary":
        dataParts.push(take());
        break;
      case "--data-urlencode": {
        const raw = take();
        const idx = raw.indexOf("=");
        dataUrlEncoded.push({
          name: idx > 0 ? raw.slice(0, idx) : "",
          value: idx > 0 ? raw.slice(idx + 1) : raw,
        });
        break;
      }
      case "-f":
      case "--form":
      case "--form-string": {
        const raw = take();
        const idx = raw.indexOf("=");
        const name = idx > 0 ? raw.slice(0, idx) : raw;
        const content = idx > 0 ? raw.slice(idx + 1) : "";
        if (content.startsWith("@")) {
          formFiles.push({ name, path: content.slice(1) });
        } else {
          form.push({ name, value: content });
        }
        sawForm = true;
        break;
      }
      case "-u":
      case "--user": {
        const raw = take();
        const idx = raw.indexOf(":");
        auth = {
          type: "basic",
          username: idx > 0 ? raw.slice(0, idx) : raw,
          password: idx > 0 ? raw.slice(idx + 1) : "",
        };
        break;
      }
      case "-b":
      case "--cookie": {
        const raw = take();
        if (!raw.startsWith("@")) {
          setHeader(spec.headers, "Cookie", raw);
        }
        break;
      }
      case "-g":
      case "--get":
        moveToQuery = true;
        break;
      default:
        // 无参 flag 直接忽略；带独立参数的 flag（如 -o file）吞掉下一个词避免误当 URL
        if (["-o", "--output", "-D", "--dump-header", "-A", "--user-agent", "-e", "--referer", "-m", "--max-time", "--connect-timeout", "--retry"].includes(flag)) {
          take();
        }
        break;
    }
  }

  if (!spec.url) {
    throw new Error("no-url");
  }

  // data 组装：--data-urlencode 优先成表单（混用 -d 时把 k=v 形态的片段并入表单）；
  // 纯 -d 拼接为 body
  if (sawForm) {
    spec.bodyType = "form-data";
    spec.form = form;
    spec.formFiles = formFiles;
  } else if (dataUrlEncoded.length) {
    spec.bodyType = "form-urlencoded";
    spec.form = [
      ...dataUrlEncoded,
      ...dataParts.map((raw) => {
        const idx = raw.indexOf("=");
        return { name: idx > 0 ? raw.slice(0, idx) : "", value: idx > 0 ? raw.slice(idx + 1) : raw };
      }),
    ];
  } else if (dataParts.length) {
    spec.body = dataParts.join("&");
    const contentType = spec.headers.find((h) => h.name.toLowerCase() === "content-type")?.value ?? "";
    spec.bodyType = /json/i.test(contentType) ? "json" : "text";
  }

  // -G（--get）：data/表单全部解码并入查询串
  if (moveToQuery) {
    const toParams = (pairs: HttpHeader[]) => {
      pairs.forEach((p) => {
        if (p.name || p.value) {
          spec.params.push({ name: p.name, value: p.value });
        }
      });
    };
    if (spec.body) {
      toParams(
        spec.body
          .split("&")
          .filter(Boolean)
          .map((pair) => {
            const idx = pair.indexOf("=");
            return {
              name: decodeURIComponent(idx > 0 ? pair.slice(0, idx) : pair),
              value: idx > 0 ? decodeURIComponent(pair.slice(idx + 1)) : "",
            };
          }),
      );
      spec.body = undefined;
    }
    if (spec.form?.length) {
      toParams(spec.form);
      spec.form = undefined;
    }
    spec.bodyType = undefined;
  }

  if (auth) {
    spec.auth = auth;
  }
  if (!spec.method) {
    spec.method = spec.body || spec.form?.length || spec.formFiles?.length ? "POST" : "GET";
  }
  // -G 强制 GET（curl 语义）
  if (moveToQuery) {
    spec.method = "GET";
  }
  spec.url = extractQueryParams(spec.url, spec.params);
  return spec;
}

/** 由请求表单生成等价 cURL 文本（多行续接格式）。 */
export function toCurl(spec: HttpRequestSpec): string {
  const parts: string[] = ["curl"];
  // API Key 为 query 落点时并入 URL 参数
  const apiKeyQuery =
    spec.auth?.type === "apikey" && (spec.auth.in ?? "header") === "query" && spec.auth.key?.trim()
      ? { name: spec.auth.key.trim(), value: spec.auth.value ?? "" }
      : null;
  const params = apiKeyQuery ? [...spec.params, apiKeyQuery] : spec.params;
  const url = params.length
    ? `${spec.url}${spec.url.includes("?") ? "&" : "?"}${params
        .map((p) => `${encodeURIComponent(p.name)}=${encodeURIComponent(p.value)}`)
        .join("&")}`
    : spec.url;
  parts.push(shQuote(url));

  if (spec.method && spec.method.toUpperCase() !== "GET") {
    parts.push("-X", spec.method.toUpperCase());
  }
  for (const header of spec.headers) {
    if (!header.name.trim()) {
      continue;
    }
    parts.push("-H", shQuote(`${header.name}: ${header.value}`));
  }
  if (spec.auth?.type === "bearer" && spec.auth.token?.trim()) {
    parts.push("-H", shQuote(`Authorization: Bearer ${spec.auth.token.trim()}`));
  } else if (spec.auth?.type === "basic" && spec.auth.username) {
    parts.push("-u", shQuote(`${spec.auth.username}:${spec.auth.password ?? ""}`));
  } else if (spec.auth?.type === "apikey" && (spec.auth.in ?? "header") === "header" && spec.auth.key?.trim()) {
    parts.push("-H", shQuote(`${spec.auth.key.trim()}: ${spec.auth.value ?? ""}`));
  }

  const bodyType = spec.bodyType ?? "json";
  if (bodyType === "form-urlencoded") {
    for (const field of spec.form ?? []) {
      parts.push("--data-urlencode", shQuote(`${field.name}=${field.value}`));
    }
  } else if (bodyType === "form-data") {
    for (const field of spec.form ?? []) {
      parts.push("-F", shQuote(`${field.name}=${field.value}`));
    }
    for (const file of spec.formFiles ?? []) {
      parts.push("-F", shQuote(`${file.name}=@${file.path}`));
    }
  } else if (spec.body) {
    parts.push("--data-raw", shQuote(spec.body));
  }
  parts.push("--compressed");
  // 续接排版：行宽不做严格限制，按参数切行保证可读
  const lines: string[] = [];
  for (let i = 0; i < parts.length; i += 2) {
    lines.push(parts.slice(i, i + 2).join(" "));
  }
  return lines.join(" \\\n  ");
}
