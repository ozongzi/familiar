import type { ToolBubble } from "../api/types";

export interface ToolArgsView {
  raw: string;
  parsed: Record<string, unknown> | null;
  command: string | null;
  script: string | null;
  path: string | null;
  oldStr: string | null;
  editContent: string | null;
  question: string | null;
  options: string[] | undefined;
}

function parseJson(raw: string): Record<string, unknown> | null {
  if (!raw) return null;
  try {
    return JSON.parse(raw) as Record<string, unknown>;
  } catch {
    return null;
  }
}

function unwrapObject(value: unknown): Record<string, unknown> | null {
  if (!value) return null;
  if (typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  if (typeof value === "string") {
    return parseJson(value);
  }
  return null;
}

function unwrapDesktopCommanderArgs(
  parsed: Record<string, unknown> | null,
): Record<string, unknown> | null {
  if (!parsed) return null;

  const candidates = [
    parsed,
    unwrapObject(parsed.arguments),
    unwrapObject(parsed.input),
    unwrapObject(parsed.params),
    unwrapObject(parsed.payload),
    unwrapObject(parsed.text),
  ];

  for (const candidate of candidates) {
    if (candidate) return candidate;
  }
  return null;
}

function getStringValue(
  args: Record<string, unknown> | null,
  raw: string,
  keys: string[],
): string | null {
  for (const key of keys) {
    const value = args?.[key];
    if (typeof value === "string" && value.length > 0) {
      return value;
    }
  }

  for (const key of keys) {
    const extracted = extractStreamingString(raw, key);
    if (extracted !== null) {
      return extracted;
    }
  }

  return null;
}

function extractStreamingString(raw: string, key: string): string | null {
  const keyPattern = new RegExp(`"${key}"\\s*:\\s*"`);
  const keyMatch = raw.match(keyPattern);
  if (!keyMatch || keyMatch.index === undefined) return null;

  const valueStart = keyMatch.index + keyMatch[0].length;
  const rest = raw.slice(valueStart);

  let value = "";
  let i = 0;
  while (i < rest.length) {
    const ch = rest[i];
    if (ch === "\\") {
      if (i + 1 < rest.length) {
        const next = rest[i + 1];
        const escapes: Record<string, string> = {
          '"': '"',
          "\\": "\\",
          "/": "/",
          b: "\b",
          f: "\f",
          n: "\n",
          r: "\r",
          t: "\t",
        };
        if (next === "u" && i + 5 < rest.length) {
          const hex = rest.slice(i + 2, i + 6);
          if (/^[0-9a-fA-F]{4}$/.test(hex)) {
            value += String.fromCharCode(parseInt(hex, 16));
            i += 6;
            continue;
          }
        }
        value += escapes[next] ?? next;
        i += 2;
      } else {
        break;
      }
    } else if (ch === '"') {
      return value;
    } else {
      value += ch;
      i++;
    }
  }

  return value.length > 0 ? value : null;
}

export function buildToolArgsView(bubble: ToolBubble): ToolArgsView {
  const raw = bubble.argsRaw || "";
  const parsed = parseJson(raw);
  const normalized = unwrapDesktopCommanderArgs(parsed);
  const isReplaceTool = bubble.name === "edit_block";
  const isWriteTool = bubble.name === "write";

  const command =
    bubble.name === "bash" || bubble.name === "execute_command"
      ? getStringValue(normalized, raw, ["command", "cmd"])
      : null;

  const script =
    bubble.name === "run_py" || bubble.name === "run_ts"
      ? getStringValue(normalized, raw, ["script", "code"])
      : null;

  const path =
    isReplaceTool || isWriteTool
      ? getStringValue(normalized, raw, ["path", "file_path", "target_file"])
      : null;

  const oldStr = isReplaceTool
    ? getStringValue(normalized, raw, ["old_str", "old_text", "oldText"])
    : null;

  const editContent = isReplaceTool
    ? getStringValue(normalized, raw, ["new_str", "new_text", "newText"])
    : isWriteTool
      ? getStringValue(normalized, raw, ["content", "file_text", "text"])
      : null;

  const question =
    bubble.name === "ask"
      ? getStringValue(normalized, raw, ["question", "prompt"])
      : null;

  const maybeOptions = normalized?.options ?? normalized?.choices;
  const options = Array.isArray(maybeOptions)
    ? maybeOptions.filter((v): v is string => typeof v === "string")
    : undefined;

  return {
    raw,
    parsed: normalized,
    command,
    script,
    path,
    oldStr,
    editContent,
    question,
    options,
  };
}
