import React, { useEffect, useMemo, useRef } from "react";
import DOMPurify from "dompurify";
import { marked } from "marked";
import { useMonaco } from "@monaco-editor/react";
import { copy_text } from "../lib/clipboard";

export interface MarkdownRendererProps {
  markdown: string;
  className?: string;
}

function escape_html(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/\"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

function safe_lang(raw: string): string {
  const value = (raw || "").trim().toLowerCase();
  if (!value) return "plaintext";
  if (!/^[a-z0-9_+-]+$/.test(value)) return "plaintext";
  return value;
}

export function MarkdownRenderer({ markdown, className }: MarkdownRendererProps): React.ReactElement {
  const monaco = useMonaco();
  const root_ref = useRef<HTMLDivElement | null>(null);
  const raw_code_by_el_ref = useRef<WeakMap<HTMLElement, string>>(new WeakMap());

  const sanitized_html = useMemo(() => {
    const md = typeof markdown === "string" ? markdown : String(markdown ?? "");

    const renderer = new marked.Renderer();
    renderer.code = ({ text, lang }: { text: string; lang?: string }) => {
      const language = safe_lang((lang || "").split(/\s+/)[0] || "");
      const safe_code = escape_html(text || "");
      return (
        `<div class="md-code-block" data-lang="${language}">` +
        `<div class="md-code-toolbar">` +
        `<span class="md-code-lang">${language}</span>` +
        `<button type="button" class="md-code-copy" data-md-copy="true">Copy</button>` +
        `</div>` +
        `<pre><code data-lang="${language}" class="language-${language}">${safe_code}</code></pre>` +
        `</div>`
      );
    };

    const raw = marked.parse(md, { gfm: true, breaks: true, renderer }) as string;

    return DOMPurify.sanitize(raw, {
      USE_PROFILES: { html: true },
      ADD_TAGS: ["button"],
      ADD_ATTR: ["data-lang", "data-md-copy"],
    });
  }, [markdown]);

  useEffect(() => {
    const root = root_ref.current;
    if (!root) return;
    if (!monaco) return;

    try {
      monaco.editor.setTheme("vs-dark");
    } catch {
      // Best-effort.
    }

    let cancelled = false;
    const nodes = Array.from(root.querySelectorAll("pre code[data-lang]")) as HTMLElement[];

    (async () => {
      for (const node of nodes) {
        if (cancelled) return;
        const lang = safe_lang(node.dataset.lang || "");
        const text = node.textContent || "";
        if (!text.trim()) continue;

        raw_code_by_el_ref.current.set(node, text);

        try {
          const html = await monaco.editor.colorize(text, lang, { tabSize: 2 });
          if (cancelled) return;
          node.innerHTML = html;
        } catch {
          // Ignore.
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [monaco, sanitized_html]);

  const on_click = async (e: React.MouseEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement | null;
    const btn = target?.closest?.("[data-md-copy]") as HTMLElement | null;
    if (!btn) return;

    const block = btn.closest(".md-code-block");
    const code_el = block?.querySelector("pre code") as HTMLElement | null;
    const text = (code_el ? raw_code_by_el_ref.current.get(code_el) : null) || code_el?.textContent || "";
    if (!text) return;

    const ok = await copy_text(text);
    btn.textContent = ok ? "Copied" : "Copy failed";
    window.setTimeout(() => {
      if (btn) btn.textContent = "Copy";
    }, 900);
  };

  return (
    <div
      ref={root_ref}
      className={className ? `markdown-body ${className}` : "markdown-body"}
      onClick={on_click}
      dangerouslySetInnerHTML={{ __html: sanitized_html }}
    />
  );
}

export default MarkdownRenderer;
