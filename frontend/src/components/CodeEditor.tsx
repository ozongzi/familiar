import { useEffect, useRef } from "react";
import Editor, { type OnMount, loader } from "@monaco-editor/react";

loader.config({ paths: { vs: "/monaco/vs" } });

export type CodeEditorLanguage = "sql" | "json" | "markdown" | "plaintext";

interface CodeEditorProps {
  value: string;
  onChange?: (next: string) => void;
  language: CodeEditorLanguage;
  height?: number | string;
  readOnly?: boolean;
  placeholder?: string;
  onSubmit?: () => void;
}

export function CodeEditor({
  value,
  onChange,
  language,
  height = 180,
  readOnly = false,
  placeholder,
  onSubmit,
}: CodeEditorProps) {
  const onSubmitRef = useRef(onSubmit);
  useEffect(() => {
    onSubmitRef.current = onSubmit;
  }, [onSubmit]);

  const handleMount: OnMount = (editor, monaco) => {
    editor.addCommand(
      monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter,
      () => onSubmitRef.current?.(),
    );
  };

  return (
    <div
      style={{
        border: "1px solid var(--border)",
        borderRadius: 8,
        overflow: "hidden",
        background: "var(--bg-secondary)",
      }}
    >
      <Editor
        height={height}
        language={language}
        value={value}
        onChange={(v) => onChange?.(v ?? "")}
        onMount={handleMount}
        theme="vs"
        loading={<div style={{ padding: 12, opacity: 0.6, fontSize: 12 }}>加载编辑器…</div>}
        options={{
          readOnly,
          minimap: { enabled: false },
          fontSize: 13,
          fontFamily: '"JetBrains Mono", "Fira Code", ui-monospace, monospace',
          lineNumbers: "on",
          scrollBeyondLastLine: false,
          wordWrap: "on",
          tabSize: 2,
          automaticLayout: true,
          padding: { top: 8, bottom: 8 },
          placeholder,
          formatOnPaste: true,
          fixedOverflowWidgets: true,
        }}
      />
    </div>
  );
}
