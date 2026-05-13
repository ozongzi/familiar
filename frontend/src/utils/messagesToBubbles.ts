import type {
  ChatBubble,
  Message,
  ToolBubble,
  UploadBubble,
} from "../api/types";

function uid() {
  return Math.random().toString(36).slice(2);
}

function tryParseWidgetArgs(raw: string): unknown {
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

function extractWidgetCode(result: unknown): string | null {
  if (typeof result === "string") return result || null;
  if (result && typeof result === "object") {
    const r = result as Record<string, unknown>;
    if (typeof r.widget_code === "string") return r.widget_code || null;
    if (typeof r.html === "string") return r.html || null;
    if (typeof r.content === "string") return r.content || null;
  }
  return null;
}

function extractDescription(raw: string): string | null {
  const m = raw.match(/"description"\s*:\s*"((?:[^"\\]|\\.)*)(")?/);
  if (!m) return null;
  const value = m[1];
  if (!value) return null;
  try {
    return JSON.parse(`"${value}"`);
  } catch {
    return value;
  }
}

/**
 * Convert raw active-branch messages into chat bubbles for read-only display.
 * Mirrors the core branches of useChat.setHistory but omits streaming-related
 * state and sandbox-image URL signing (callers without an auth token cannot
 * fetch /api/files anyway, so sandbox image refs are dropped).
 */
export function messagesToBubbles(msgs: Message[]): ChatBubble[] {
  const toolResultMap = new Map<string, unknown>();
  const toolImagesMap = new Map<string, string[]>();

  for (const m of msgs) {
    if (m.role !== "tool" || !m.tool_call_id || !m.content) continue;
    let parsed: unknown = m.content;
    try {
      const outer = JSON.parse(m.content);
      if (Array.isArray(outer)) {
        const textBlock = (outer as unknown[]).find(
          (b): b is { type: string; text: string } =>
            b !== null &&
            typeof b === "object" &&
            (b as Record<string, unknown>).type === "text" &&
            typeof (b as Record<string, unknown>).text === "string",
        );
        if (textBlock) {
          try {
            parsed = JSON.parse(textBlock.text);
          } catch {
            parsed = textBlock.text;
          }
        } else {
          parsed = outer;
        }

        // Image parts: only keep base64 inline ones — sandbox refs require
        // an auth token to fetch, which a public viewer doesn't have.
        type ImageBlock = {
          type: "image";
          data: { url?: string; base64?: string };
          mime_type: string;
        };
        const imageUrls = (outer as unknown[])
          .filter(
            (b): b is ImageBlock =>
              b !== null &&
              typeof b === "object" &&
              (b as Record<string, unknown>).type === "image",
          )
          .map((b) => {
            if (b.data?.base64) {
              return `data:${b.mime_type};base64,${b.data.base64}`;
            }
            const raw = b.data?.url ?? "";
            if (raw.startsWith("__sandbox__:")) return "";
            return raw;
          })
          .filter(Boolean);
        if (imageUrls.length > 0) {
          toolImagesMap.set(m.tool_call_id, imageUrls);
        }
      } else {
        parsed = outer;
      }
    } catch {
      /* leave as string */
    }
    toolResultMap.set(m.tool_call_id, parsed);
  }

  const out: ChatBubble[] = [];
  const consumedMsgIds = new Set<number>();

  for (let mi = 0; mi < msgs.length; mi++) {
    const m = msgs[mi];
    if (m.role === "system" || m.role === "tool") continue;
    if (consumedMsgIds.has(m.id)) continue;

    if (m.role === "assistant" && m.tool_calls) {
      type RawToolCall = { id: string; name: string; arguments: string };
      let calls: RawToolCall[] = [];
      try {
        calls = JSON.parse(m.tool_calls) as RawToolCall[];
      } catch {
        /* skip */
      }

      const hasText = !!(m.content && m.content.trim().length > 0);
      const hasReasoning = !!(m.reasoning && m.reasoning.trim().length > 0);
      if (hasText || hasReasoning) {
        out.push({
          kind: "text",
          key: uid(),
          role: "assistant",
          content: m.content ?? "",
          reasoning: m.reasoning ?? "",
          streaming: false,
          msgId: m.id,
          siblings: m.siblings,
          summaryStartId: m.summary_start_id,
        });
      }

      for (const tc of calls) {
        const { id, name, arguments: argsRaw = "" } = tc;
        if (!id || !name) continue;
        let result = toolResultMap.get(id) ?? null;

        const isAskMarker =
          result !== null &&
          typeof result === "object" &&
          (result as Record<string, unknown>).__ask__ === true;
        if (name === "ask" && (result === null || isAskMarker)) {
          result = null;
          for (let j = mi + 1; j < msgs.length; j++) {
            const next = msgs[j];
            if (next.role !== "user") continue;
            const c = next.content;
            if (!c || !c.trim()) continue;
            let isFileUpload = false;
            try {
              const p = JSON.parse(c) as Record<string, unknown>;
              if (p.__type === "file_upload") isFileUpload = true;
            } catch {
              /* not JSON */
            }
            if (isFileUpload || c.startsWith("__multimodal__:")) break;
            result = { answer: c };
            consumedMsgIds.add(next.id);
            break;
          }
        }

        const widgetCode =
          name === "visualize" && result !== null
            ? (extractWidgetCode(result) ??
              extractWidgetCode(tryParseWidgetArgs(argsRaw)))
            : null;

        let diagramCode: string | undefined;
        if (name === "diagram") {
          try {
            const parsed = JSON.parse(argsRaw) as Record<string, unknown>;
            if (typeof parsed.code === "string") diagramCode = parsed.code;
          } catch {
            /* ignore */
          }
        }

        const historyImages = toolImagesMap.get(id);
        const toolBubble: ToolBubble = {
          kind: "tool",
          key: `tool-${id}`,
          role: "tool",
          name,
          description: extractDescription(argsRaw) ?? "",
          argsRaw,
          result,
          pending: result === null,
          ...(widgetCode ? { widgetCode } : {}),
          ...(diagramCode ? { diagramCode } : {}),
          ...(historyImages && historyImages.length > 0
            ? { images: historyImages }
            : {}),
        };
        out.push(toolBubble);
      }
      continue;
    }

    const hasContent = !!(m.content && m.content.trim().length > 0);
    const hasAssistantReasoningOnly =
      m.role === "assistant" &&
      !!(m.reasoning && m.reasoning.trim().length > 0);
    if (
      (m.role === "user" || m.role === "assistant") &&
      (hasContent || hasAssistantReasoningOnly)
    ) {
      const content = m.content ?? "";

      if (m.role === "user") {
        // file upload JSON marker
        try {
          const parsed = JSON.parse(content) as Record<string, unknown>;
          if (
            parsed.__type === "file_upload" &&
            typeof parsed.filename === "string" &&
            typeof parsed.path === "string" &&
            typeof parsed.size === "number"
          ) {
            const upload: UploadBubble = {
              kind: "upload",
              key: uid(),
              role: "user",
              filename: parsed.filename,
              path: parsed.path,
              size: parsed.size,
            };
            out.push(upload);
            continue;
          }
        } catch {
          /* not JSON */
        }

        // Multimodal: "__multimodal__:[...]"
        if (content.startsWith("__multimodal__:")) {
          const json = content.slice("__multimodal__:".length);
          try {
            type Part =
              | { type: "text"; text: string }
              | {
                  type: "image";
                  data: { base64?: string; url?: string };
                  mime_type: string;
                };
            const parts = JSON.parse(json) as Part[];

            const textContent = parts
              .filter((p): p is Extract<Part, { type: "text" }> => p.type === "text")
              .map((p) => p.text)
              .join("");

            try {
              const fp = JSON.parse(textContent) as Record<string, unknown>;
              if (
                fp.__type === "file_upload" &&
                typeof fp.filename === "string" &&
                typeof fp.path === "string" &&
                typeof fp.size === "number"
              ) {
                out.push({
                  kind: "upload",
                  key: uid(),
                  role: "user",
                  filename: fp.filename,
                  path: fp.path,
                  size: fp.size,
                });
                continue;
              }
            } catch {
              /* text is not file_upload */
            }

            const images = parts
              .filter(
                (p): p is Extract<Part, { type: "image" }> => p.type === "image",
              )
              .map((p) =>
                p.data.base64
                  ? `data:${p.mime_type};base64,${p.data.base64}`
                  : (p.data.url ?? ""),
              )
              .filter(Boolean);

            out.push({
              kind: "text",
              key: uid(),
              role: "user",
              content: textContent,
              reasoning: "",
              streaming: false,
              images: images.length > 0 ? images : undefined,
              msgId: m.id,
              siblings: m.siblings,
            });
            continue;
          } catch {
            /* fall through to plain text */
          }
        }
      }

      out.push({
        kind: "text",
        key: uid(),
        role: m.role as "user" | "assistant",
        content,
        reasoning: m.reasoning ?? "",
        streaming: false,
        msgId: m.id,
        siblings: m.siblings,
        summaryStartId: m.summary_start_id,
      });
    }
  }

  return out;
}
