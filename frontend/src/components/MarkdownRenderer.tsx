import { memo, useCallback, useState, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkBreaks from "remark-breaks";
import rehypeKatex from "rehype-katex";
import rehypeHighlight from "rehype-highlight";

interface Props {
  content: string;
  className?: string;
}

// ─── Copy button (React, not imperative) ─────────────────────────────────────

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);

  const onClick = useCallback(() => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }, [text]);

  return (
      <button
          type="button"
          className={`code-copy-btn${copied ? " copied" : ""}`}
          onClick={onClick}
          aria-label="复制代码"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none"
             stroke="currentColor" strokeWidth="2" strokeLinecap="round"
             strokeLinejoin="round" aria-hidden="true">
          <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
          <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
        </svg>
        <span>{copied ? "已复制" : "复制"}</span>
      </button>
  );
}

// ─── Code block wrapper ──────────────────────────────────────────────────────
// react-markdown gives us the <pre><code class="language-xxx hljs">...</code></pre>
// subtree already syntax-highlighted by rehype-highlight. We wrap it in our
// chrome (header + copy button) matching the existing .code-block CSS.

function extractText(node: ReactNode): string {
  if (node == null || typeof node === "boolean") return "";
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(extractText).join("");
  if (typeof node === "object" && "props" in node) {
    return extractText((node as { props: { children?: ReactNode } }).props.children);
  }
  return "";
}

function extractLanguage(node: ReactNode): string {
  if (node == null || typeof node === "boolean") return "";
  if (Array.isArray(node)) {
    for (const child of node) {
      const lang = extractLanguage(child);
      if (lang) return lang;
    }
    return "";
  }
  if (typeof node === "object" && "props" in node) {
    const props = (node as { props: { className?: string; children?: ReactNode } }).props;
    const m = /language-([\w-]+)/.exec(props.className ?? "");
    if (m) return m[1];
    return extractLanguage(props.children);
  }
  return "";
}

const components: Components = {
  pre({ children }) {
    const lang = extractLanguage(children);
    const raw = extractText(children);
    return (
        <div className="code-block">
          <div className="code-header">
            {lang ? <span className="code-lang">{lang}</span> : <span className="code-lang" />}
            <CopyButton text={raw} />
          </div>
          <pre className="code-pre">{children}</pre>
        </div>
    );
  },
  code({ className, children, ...rest }) {
    // Inline code: no language class, no <pre> parent (handled by `pre`).
    const isInline = !/language-/.test(className ?? "");
    if (isInline) {
      return <code className="inline-code">{children}</code>;
    }
    // Block code: let rehype-highlight's className (language-xxx, hljs) pass through.
    return (
        <code className={className} {...rest}>
          {children}
        </code>
    );
  },
  a({ href, children }) {
    return (
        <a href={href} target="_blank" rel="noopener noreferrer">
          {children}
        </a>
    );
  },
};

// ─── Component ────────────────────────────────────────────────────────────────

export const MarkdownRenderer = memo(function MarkdownRenderer({
  content,
  className,
}: Props) {
  return (
      <div className={`prose${className ? ` ${className}` : ""}`}>
        <ReactMarkdown
            remarkPlugins={[remarkGfm, remarkBreaks, remarkMath]}
            rehypePlugins={[
              [rehypeHighlight, { detect: true, ignoreMissing: true }],
              [rehypeKatex, { throwOnError: false, output: "html" }],
            ]}
            components={components}
        >
          {content}
        </ReactMarkdown>
      </div>
  );
});
