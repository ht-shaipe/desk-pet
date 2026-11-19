/**
 * 极简 JSONPath 求值：覆盖调试常用子集。
 *
 * 支持：`$` 根、`.key` / `['key']` 属性、`[index]` 数组下标、`[*]` / `.*` 通配、
 * `..key` 递归下降。不支持过滤表达式 `[?(...)]` 与脚本表达式。
 * 结果恒为数组（命中 0 个为空数组），与主流 JSONPath 语义一致。
 */

/** 递归收集所有子孙节点中 key 匹配的值 */
function collectDescendants(node: unknown, key: string, out: unknown[]): void {
  if (Array.isArray(node)) {
    for (const item of node) {
      collectDescendants(item, key, out);
    }
  } else if (node && typeof node === "object") {
    const record = node as Record<string, unknown>;
    if (Object.prototype.hasOwnProperty.call(record, key)) {
      out.push(record[key]);
    }
    for (const value of Object.values(record)) {
      collectDescendants(value, key, out);
    }
  }
}

/** 单步取值：对象属性（含通配）或数组下标/通配 */
function step(node: unknown, key: string, wildcard: boolean, out: unknown[]): void {
  if (Array.isArray(node)) {
    if (wildcard) {
      out.push(...node);
    } else {
      const index = Number(key);
      if (Number.isInteger(index) && index >= 0 && index < node.length) {
        out.push(node[index]);
      }
    }
    return;
  }
  if (node && typeof node === "object") {
    const record = node as Record<string, unknown>;
    if (wildcard) {
      out.push(...Object.values(record));
    } else if (Object.prototype.hasOwnProperty.call(record, key)) {
      out.push(record[key]);
    }
  }
}

/** 解析一段路径 token 序列（去掉起始 `$`） */
function tokenize(path: string): Array<{ key: string; wildcard: boolean; deep: boolean }> {
  const tokens: Array<{ key: string; wildcard: boolean; deep: boolean }> = [];
  let rest = path.trim().replace(/^\$\.?/, "");
  const re = /\.\.([A-Za-z0-9_$-]+)|\.([A-Za-z0-9_$*-]+)|\['([^']+)'\]|\[(\*|-?\d+)\]/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(rest)) !== null) {
    if (match[1] !== undefined) {
      tokens.push({ key: match[1], wildcard: false, deep: true });
    } else if (match[2] !== undefined) {
      tokens.push({ key: match[2], wildcard: match[2] === "*", deep: false });
    } else if (match[3] !== undefined) {
      tokens.push({ key: match[3], wildcard: false, deep: false });
    } else {
      tokens.push({ key: match[4], wildcard: match[4] === "*", deep: false });
    }
  }
  return tokens;
}

/**
 * 求值 JSONPath；语法无法解析或求值出错时返回 null（调用方展示原样文本）。
 */
export function evaluateJsonPath(root: unknown, path: string): unknown[] | null {
  const trimmed = path.trim();
  if (!trimmed.startsWith("$")) {
    return null;
  }
  const tokens = tokenize(trimmed);
  let current: unknown[] = [root];
  for (const token of tokens) {
    const next: unknown[] = [];
    for (const node of current) {
      if (token.deep) {
        collectDescendants(node, token.key, next);
      } else {
        step(node, token.key, token.wildcard, next);
      }
    }
    current = next;
    if (!current.length) {
      return [];
    }
  }
  return current;
}
