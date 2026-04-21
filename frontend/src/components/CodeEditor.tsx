import { useEffect, useRef } from "react";
import { EditorView, keymap, lineNumbers, drawSelection, dropCursor, highlightActiveLine, highlightActiveLineGutter, rectangularSelection, crosshairCursor, placeholder as placeholderExtension } from "@codemirror/view";
import { EditorState, Compartment } from "@codemirror/state";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { syntaxHighlighting, defaultHighlightStyle, bracketMatching } from "@codemirror/language";
import { sql, StandardSQL } from "@codemirror/lang-sql";
import { markdown } from "@codemirror/lang-markdown";
import { json } from "@codemirror/lang-json";

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

const theme = EditorView.theme({
  "&": {
    fontSize: "13px",
    fontFamily: '"JetBrains Mono", "Fira Code", ui-monospace, monospace',
    backgroundColor: "var(--bg-secondary)",
  },
  ".cm-content": {
    caretColor: "var(--text-primary)",
    color: "var(--text-primary)",
  },
  ".cm-cursor": {
    borderLeftColor: "var(--text-primary)",
  },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
    backgroundColor: "var(--accent-muted, rgba(100, 150, 255, 0.2)) !important",
  },
  ".cm-gutters": {
    backgroundColor: "var(--bg-secondary)",
    color: "var(--text-muted, #888)",
    border: "none",
  },
  ".cm-activeLineGutter": {
    backgroundColor: "var(--bg-tertiary, rgba(255,255,255,0.05))",
  },
  ".cm-activeLine": {
    backgroundColor: "var(--bg-tertiary, rgba(255,255,255,0.05))",
  },
  ".cm-placeholder": {
    color: "var(--text-muted, #888)",
    fontStyle: "italic",
  },
}, { dark: true });

function getLanguageExtension(lang: CodeEditorLanguage) {
  switch (lang) {
    case "sql":
      return sql({ dialect: StandardSQL });
    case "json":
      return json();
    case "markdown":
      return markdown();
    case "plaintext":
    default:
      return [];
  }
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
  const containerRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onSubmitRef = useRef(onSubmit);
  const languageConf = useRef(new Compartment());
  const readOnlyConf = useRef(new Compartment());
  const valueConf = useRef(new Compartment());

  useEffect(() => {
    onSubmitRef.current = onSubmit;
  }, [onSubmit]);

  useEffect(() => {
    if (!containerRef.current || viewRef.current) return;

    const updateListener = EditorView.updateListener.of((update) => {
      if (update.docChanged && onChange) {
        onChange(update.state.doc.toString());
      }
    });

    const submitKeymap = keymap.of([{
      key: "Mod-Enter",
      run: () => {
        onSubmitRef.current?.();
        return true;
      },
    }]);

    const state = EditorState.create({
      doc: value,
      extensions: [
        lineNumbers(),
        history(),
        drawSelection(),
        dropCursor(),
        EditorState.allowMultipleSelections.of(true),
        rectangularSelection(),
        crosshairCursor(),
        highlightActiveLine(),
        highlightActiveLineGutter(),
        keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
        bracketMatching(),
        syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
        theme,
        updateListener,
        submitKeymap,
        languageConf.current.of(getLanguageExtension(language)),
        readOnlyConf.current.of(EditorState.readOnly.of(readOnly)),
        valueConf.current.of(EditorView.contentAttributes.of({})),
                placeholder ? placeholderExtension(placeholder) : [],
        EditorView.lineWrapping,
      ],
    });

    const view = new EditorView({
      state,
      parent: containerRef.current,
    });

    viewRef.current = view;

    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, []);

  // Update language
  useEffect(() => {
    if (viewRef.current) {
      viewRef.current.dispatch({
        effects: languageConf.current.reconfigure(getLanguageExtension(language)),
      });
    }
  }, [language]);

  // Update readOnly
  useEffect(() => {
    if (viewRef.current) {
      viewRef.current.dispatch({
        effects: readOnlyConf.current.reconfigure(EditorState.readOnly.of(readOnly)),
      });
    }
  }, [readOnly]);

  // Update value from outside
  useEffect(() => {
    if (viewRef.current) {
      const currentDoc = viewRef.current.state.doc.toString();
      if (currentDoc !== value) {
        viewRef.current.dispatch({
          changes: { from: 0, to: currentDoc.length, insert: value },
        });
      }
    }
  }, [value]);

  return (
    <div
      style={{
        border: "1px solid var(--border)",
        borderRadius: 8,
        overflow: "hidden",
        background: "var(--bg-secondary)",
      }}
    >
      <div
        ref={containerRef}
        style={{
          height: typeof height === "number" ? `${height}px` : height,
          overflow: "auto",
        }}
      />
    </div>
  );
}

