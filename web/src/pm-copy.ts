import remarkParse from "remark-parse";
import { unified } from "unified";

const markdown = unified().use(remarkParse);

type Node = {
  type: string;
  children?: Node[];
  value?: string;
  url?: string;
  position?: { start: { offset?: number }; end: { offset?: number } };
};

function escapeHtml(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function draftHtml(text: string, lineStart: boolean): string {
  return escapeHtml(text).replace(/^[ \t]+/gm, (indent, index: number) => (
    index > 0 || lineStart ? "&nbsp;".repeat(indent.replace(/\t/g, "    ").length) : indent
  )).replace(/\n/g, "<br>");
}

function labelText(node: Node): string {
  return node.value ?? node.children?.map(labelText).join("") ?? "";
}

function safeLink(url: string): boolean {
  if (!/^https?:\/\//i.test(url) || /[\s<>\\\p{Cc}]/u.test(url)) return false;
  try {
    const address = new URL(url);
    return Boolean(address.hostname) && !address.username && !address.password
      && !url.slice(url.indexOf("//") + 2).split(/[/?#]/, 1)[0].includes("@");
  } catch {
    return false;
  }
}

export function slackCopy(body: string): { text: string; html?: string } {
  const draft = body.trimEnd();
  let offset = 0;
  let text = "";
  let html = "";
  function visit(node: Node) {
    if (node.type !== "link") {
      node.children?.forEach(visit);
      return;
    }
    const start = node.position?.start.offset;
    const end = node.position?.end.offset;
    const url = node.url;
    if (start === undefined || end === undefined || start < offset || end <= start || end > draft.length || !url || !safeLink(url)) return;
    const label = labelText(node).trim();
    const before = draft.slice(offset, start);
    text += before + (label && label !== url ? `${label} <${url}>` : `<${url}>`);
    html += draftHtml(before, offset === 0 || draft[offset - 1] === "\n") + `<a href="${escapeHtml(url)}">${escapeHtml(label || url)}</a>`;
    offset = end;
  }
  visit(markdown.parse(draft));
  if (offset === 0) return { text: draft };
  const tail = draft.slice(offset);
  return { text: text + tail, html: html + draftHtml(tail, draft[offset - 1] === "\n") };
}
