import { useEffect, useRef } from "react";
import mbStyles from "./MessageBubble.module.css";
import sharedStyles from "./ToolShared.module.css";

export function FilePreviewContent({
  content,
  lang,
  lineCount,
  compact = false,
}: {
  content: string;
  lang: string;
  lineCount: number;
  compact?: boolean;
}) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    import("highlight.js").then((hljs) => {
      const el = containerRef.current?.querySelector("code");
      if (!el) return;
      if (lang && hljs.default.getLanguage(lang)) {
        el.innerHTML = hljs.default.highlight(content, { language: lang }).value;
      } else {
        el.innerHTML = hljs.default.highlightAuto(content).value;
      }
    });
  }, [content, lang]);

  if (compact) {
    return (
      <div ref={containerRef} className={sharedStyles.codePreview}>
        <pre className={sharedStyles.codePreviewPre}>
          <code className={`hljs ${lang ? `language-${lang}` : ""}`}>{content}</code>
        </pre>
      </div>
    );
  }

  return (
    <div ref={containerRef} className={mbStyles.filePreviewCode}>
      <div className={mbStyles.filePreviewCodeHeader}>
        {lang && <span className={mbStyles.filePreviewLang}>{lang}</span>}
        <span className={mbStyles.filePreviewLines}>{lineCount} 行</span>
      </div>
      <pre className={mbStyles.filePreviewPre}>
        <code className={`hljs ${lang ? `language-${lang}` : ""}`}>{content}</code>
      </pre>
    </div>
  );
}
