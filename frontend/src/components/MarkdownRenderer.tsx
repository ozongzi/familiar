import { useEffect, useMemo, useRef } from "react";
import { marked, type Renderer } from "marked";
import markedKatex from "marked-katex-extension";
import hljs from "highlight.js";
import DOMPurify from "dompurify";

interface Props {
  content: string;
  className?: string;
}

// ─── Custom marked renderer ───────────────────────────────────────────────────

function buildRenderer(): Partial<Renderer> {
  return {
    code({ text, lang }) {
      const language = lang && hljs.getLanguage(lang) ? lang : "";
      const highlighted = language
          ? hljs.highlight(text, { language }).value
          : text
              .replace(/&/g, "&amp;")
              .replace(/</g, "&lt;")
              .replace(/>/g, "&gt;");

      const langLabel = language
          ? `<span class="code-lang">${language}</span>`
          : "";

      // The copy button uses a data attribute; the actual click handler is
      // wired up imperatively in a useEffect after the HTML is injected.
      return `
<div class="code-block">
  <div class="code-header">
    ${langLabel}
    <button class="code-copy-btn" data-code="${encodeURIComponent(text)}" aria-label="复制代码">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none"
        stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
        aria-hidden="true">
        <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
        <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
      </svg>
      <span>复制</span>
    </button>
  </div>
  <pre class="code-pre"><code class="hljs ${language ? `language-${language}` : ""}">${highlighted}</code></pre>
</div>`.trim();
    },

    codespan({ text }) {
      return `<code class="inline-code">${text}</code>`;
    },
  };
}

// Build once, reuse across renders.
const renderer = buildRenderer();
marked.use(markedKatex({ throwOnError: false, output: "html" }));
marked.use({
  renderer,
  breaks: true,
  gfm: true,
});

// ─── Component ────────────────────────────────────────────────────────────────

export function MarkdownRenderer({ content, className }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);

  const html = useMemo(() => {
    const raw = marked.parse(content) as string;
    return DOMPurify.sanitize(raw, {
      ALLOWED_TAGS: [
        "p",
        "br",
        "strong",
        "em",
        "del",
        "s",
        "code",
        "pre",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "ul",
        "ol",
        "li",
        "blockquote",
        "hr",
        "a",
        "img",
        "table",
        "thead",
        "tbody",
        "tr",
        "th",
        "td",
        "span",
        "div",
        "button",
        "annotation",
        "semantics",
        "math",
        "mrow",
        "mi",
        "mn",
        "mo",
        "msup",
        "msub",
        "mfrac",
        "msqrt",
        "mtable",
        "mtr",
        "mtd",
        "svg",
        "path",
        "rect",
      ],
      ALLOWED_ATTR: [
        "href",
        "src",
        "alt",
        "title",
        "class",
        "target",
        "rel",
        "width",
        "height",
        "viewBox",
        "fill",
        "stroke",
        "stroke-width",
        "stroke-linecap",
        "stroke-linejoin",
        "aria-label",
        "aria-hidden",
        "x",
        "y",
        "rx",
        "ry",
        "d",
        "data-code",
        "style",
      ],
      FORCE_BODY: false,
      RETURN_DOM_FRAGMENT: false,
    });
  }, [content]);

  // Wire up copy buttons after DOM injection.
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const buttons =
        container.querySelectorAll<HTMLButtonElement>(".code-copy-btn");

    function handleCopy(e: MouseEvent) {
      const btn = e.currentTarget as HTMLButtonElement;
      const encoded = btn.getAttribute("data-code") ?? "";
      const code = decodeURIComponent(encoded);
      navigator.clipboard.writeText(code).then(() => {
        const span = btn.querySelector("span");
        if (!span) return;
        const original = span.textContent;
        span.textContent = "已复制";
        btn.classList.add("copied");
        setTimeout(() => {
          span.textContent = original;
          btn.classList.remove("copied");
        }, 2000);
      });
    }

    buttons.forEach((btn) => btn.addEventListener("click", handleCopy));
    return () => {
      buttons.forEach((btn) => btn.removeEventListener("click", handleCopy));
    };
  }, [html]);

  return (
      <div
          ref={containerRef}
          className={`prose${className ? ` ${className}` : ""}`}
          dangerouslySetInnerHTML={{ __html: html }}
      />
  );
}