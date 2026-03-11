// ─── Auth ─────────────────────────────────────────────────────────────────

export interface LoginRequest {
  name: string;
  password: string;
}

export interface LoginResponse {
  token: string;
}

export interface RegisterRequest {
  name: string;
  password: string;
}

export interface RegisterResponse {
  id: string;
  name: string;
  is_admin: boolean;
  created_at: string;
}

export interface MeResponse {
  id: string;
  name: string;
  is_admin: boolean;
  created_at: string;
}

// ─── Conversations ────────────────────────────────────────────────────────

export interface Conversation {
  id: string;
  name: string;
  created_at: string;
}

export interface CreateConversationRequest {
  name?: string;
}

export interface RenameConversationRequest {
  name: string;
}

// ─── Messages ─────────────────────────────────────────────────────────────

export interface Message {
  id: number;
  role: "user" | "assistant" | "system" | "tool";
  name: string | null;
  content: string | null;
  tool_calls: string | null;
  tool_call_id: string | null;
  is_summary: boolean;
  created_at: number;
}

// ─── WebSocket events ─────────────────────────────────────────────────────

export type WsServerEvent =
  | { type: "token"; content: string }
  | { type: "reasoning_token"; content: string }
  | { type: "tool_call"; id: string; name: string; delta: string }
  | { type: "tool_result"; id: string; name: string; result: unknown }
  | { type: "user_interrupt"; content: string }
  | { type: "aborted" }
  | { type: "done" }
  | { type: "error"; message: string };

export type WsClientMsg =
  | { token: string }
  | { content: string }
  | { type: "interrupt"; content: string }
  | { type: "answer"; content: string }
  | { type: "abort" };

// ─── UI-only chat bubble ──────────────────────────────────────────────────

export type BubbleRole = "user" | "assistant" | "tool";

export interface TextBubble {
  kind: "text";
  key: string;
  role: "user" | "assistant";
  content: string;
  reasoning: string;
  streaming: boolean;
}

export interface ToolBubble {
  kind: "tool";
  key: string;
  role: "tool";
  name: string;
  /** Accumulated args JSON string; complete (parseable) once all chunks arrive. */
  argsRaw: string;
  result: unknown | null;
  /** Still waiting for the tool_result event */
  pending: boolean;
}

export type ChatBubble = TextBubble | ToolBubble;

// ─── API error shape ──────────────────────────────────────────────────────

export interface ApiError {
  error: string;
}
