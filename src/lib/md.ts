import MarkdownIt from "markdown-it";

/** 共享渲染实例；html:false 使内嵌 HTML 被转义，聊天内容无需额外净化。 */
const md = new MarkdownIt({
  html: false,
  /** 单个换行也渲染为换行，聊天流式输出更接近所见即所得 */
  breaks: true,
  linkify: true,
});

/** 将 Markdown 文本渲染为安全的 HTML（供 v-html 展示）。 */
export function renderMarkdown(text: string): string {
  if (!text) {
    return "";
  }
  return md.render(text);
}
