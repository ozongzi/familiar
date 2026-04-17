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
  email?: string | null;
  display_name?: string | null;
  avatar_path?: string | null;
  is_admin: boolean;
  last_login_at?: string | null;
  created_at: string;
}

// ─── Conversations ────────────────────────────────────────────────────────

export interface Conversation {
  id: string;
  name: string;
  model_id: string | null;
  created_at: string;
}

export interface CreateConversationRequest {
  name?: string;
  model_id?: string | null;
}

// ─── Models ───────────────────────────────────────────────────────────────

export type ModelKind = "api" | "claude-code";

export interface Model {
  id: string;
  scope: "global" | "user";
  label: string;
  provider: Provider;
  model_name: string;
  api_base: string;
  is_default: boolean;
  role: "cheap" | "embedding" | null;
  visible: boolean;
  kind: ModelKind;
  admin_only: boolean;
  created_at: string;
}

export interface UpsertModelRequest {
  label: string;
  provider: Provider;
  model_name: string;
  api_base?: string;
  api_key?: string;
  extra_body?: Record<string, unknown>;
  kind?: ModelKind;
  role?: "cheap" | "embedding" | null;
  visible?: boolean;
  is_default?: boolean;
  admin_only?: boolean;
}

export interface RenameConversationRequest {
  name: string;
}

// ─── Search ──────────────────────────────────────────────────────────────

export interface SearchResult {
  id: number;
  conversation_id: string;
  conversation_name: string;
  role: string;
  content: string | null;
  created_at: number;
}

// ─── Messages ─────────────────────────────────────────────────────────────

export interface Message {
  id: number;
  role: "user" | "assistant" | "system" | "tool";
  name: string | null;
  content: string | null;
  tool_calls: string | null;
  tool_call_id: string | null;
  reasoning: string | null;
  is_summary: boolean;
  created_at: number;
  streaming: boolean;
}

// ─── WebSocket events ─────────────────────────────────────────────────────

export type WsServerEvent =
  | { type: "token"; content: string; source?: "spawn" }
  | { type: "reasoning_token"; content: string }
  | {
      type: "tool_call";
      id: string;
      name: string;
      delta: string;
      source?: "spawn";
    }
  | {
      type: "tool_result";
      id: string;
      name: string;
      args?: string;
      result: unknown;
      images?: Array<{ url: string; mime_type: string }>;
      source?: "spawn";
    }
  | { type: "tool_progress"; id: string; name: string; progress: string }
  | { type: "partial_sync"; content: string; reasoning: string }
  | { type: "user_interrupt"; content: string }
  | { type: "ask"; question: string; description?: string; options?: string[] }
  | { type: "aborted" }
  | { type: "done" }
  | { type: "error"; message: string };

export type BubbleRole = "user" | "assistant" | "tool";

export interface TextBubble {
  kind: "text";
  key: string;
  role: "user" | "assistant";
  content: string;
  reasoning: string;
  streaming: boolean;
  images?: string[];
  msgId?: number;
}

export type SpawnEvent =
  | { kind: "tool"; bubble: ToolBubble }
  | { kind: "text"; key: string; content: string };

export interface ToolBubble {
  kind: "tool";
  key: string;
  role: "tool";
  name: string;
  description: string;
  argsRaw: string;
  result: unknown | null;
  pending: boolean;
  spawnEvents?: SpawnEvent[];
  widgetCode?: string;
  widgetLoadingMessages?: string[];
  diagramCode?: string;
  _rawArgs?: string;
  progressLines?: string[];
  images?: string[];
}

export interface UploadBubble {
  kind: "upload";
  key: string;
  role: "user";
  filename: string;
  path: string;
  size: number;
  conversationId?: string;
}

export type ChatBubble = TextBubble | ToolBubble | UploadBubble;

/** 渲染用：连续工具调用合并成一个组 */
export type BubbleGroup =
  | { kind: "single"; bubble: TextBubble | UploadBubble }
  | { kind: "tools"; bubbles: ToolBubble[] };

// ─── MCPs ─────────────────────────────────────────────────────────────────

export interface Mcp {
  id: string;
  name: string;
  type: "http" | "stdio";
  config: Record<string, unknown>;
  created_at: string;
}

export interface CreateMcpRequest {
  name: string;
  type: "http" | "stdio";
  config: Record<string, unknown>;
}

// ─── Settings ─────────────────────────────────────────────────────────────

export interface UserSettings {
  mode: "custom" | "default";
  api_key: string | null;
  api_base: string | null;
  model_name: string | null;
  provider: Provider | null;
  system_prompt: string | null;
}

export type Provider = "deepseek" | "openai" | "anthropic" | "gemini" | "kimi" | "glm" | "minimax" | "grok" | "openrouter";

export interface ModelConfig {
  name: string;
  api_key: string;
  api_base: string;
  provider: Provider;
  extra_body: Record<string, unknown>;
}

export interface UpdateSettingsRequest {
  mode: "custom" | "default";
  api_key?: string | null;
  api_base?: string | null;
  model_name?: string | null;
  provider?: Provider | null;
  system_prompt?: string | null;
}

export type McpServerConfig =
  | { name: string; command: string; args?: string[]; env?: Record<string, string> }
  | { name: string; url: string };

export interface McpCatalogEntry {
  name: string;
  description: string;
  command: string;
  args: string[];
}

export interface CatalogEntry {
  id: string;
  name: string;
  description: string;
  command: string;
  args: string[];
  created_at: string;
}

export interface CatalogEntryRequest {
  name: string;
  description?: string;
  command: string;
  args?: string[];
}

export interface AdminConfig {
  mcp: McpServerConfig[];
  tavily_api_key?: string | null;
  siliconflow_api_key?: string | null;
  fal_api_key?: string | null;
}

// ─── API error shape ─────────────────────────────────────────────────────

export interface ApiError {
  error: string;
}

// ─── Skills ──────────────────────────────────────────────────────────────

export interface Skill {
  id: string;
  name: string;
  description?: string | null;
  content: string;
  created_at: string;
}

export interface CreateSkillRequest {
  name: string;
  description?: string | null;
  content: string;
}

export interface AppSkill {
  id: string;
  name: string;
  description?: string | null;
  content: string;
  created_at: string;
  updated_at: string;
}

// ─── User Management (Admin) ─────────────────────────────────────────────

export interface User {
  id: string;
  name: string;
  email?: string | null;
  display_name?: string | null;
  avatar_path?: string | null;
  is_admin: boolean;
  last_login_at?: string | null;
  created_at: string;
}

export interface UsersPage {
  items: User[];
  total: number;
  page: number;
  per_page: number;
}

export interface CreateUserRequest {
  name: string;
  email?: string | null;
  display_name?: string | null;
  password: string;
  is_admin?: boolean;
}

export interface UpdateUserRequest {
  email?: string | null;
  display_name?: string | null;
  is_admin?: boolean;
}

export interface ResetPasswordRequest {
  new_password: string;
}

// ─── Audit Logs ──────────────────────────────────────────────────────────

export interface AuditLog {
  id: string;
  user_id?: string | null;
  user_name?: string | null;
  target_user_id?: string | null;
  target_user_name?: string | null;
  action: string;
  details?: Record<string, unknown> | null;
  ip_address?: string | null;
  created_at: string;
}

export interface AuditLogPage {
  items: AuditLog[];
  total: number;
  page: number;
  per_page: number;
}

export interface AuditLogQuery {
  page?: number;
  per_page?: number;
  user_id?: string;
  target_user_id?: string;
  action?: string;
  start_date?: string;
  end_date?: string;
}

// ─── Profile ─────────────────────────────────────────────────────────────

export interface UpdateProfileRequest {
  email?: string | null;
  display_name?: string | null;
}

export interface UpdatePasswordRequest {
  current_password: string;
  new_password: string;
}

// ─── Global MCPs ─────────────────────────────────────────────────────────

export type GlobalMcp = Mcp; // Reuse Mcp interface

export interface CreateGlobalMcpRequest {
  name: string;
  type: "http" | "stdio";
  config: Record<string, unknown>;
}

export interface UpdateGlobalMcpRequest {
  name?: string;
  type?: "http" | "stdio";
  config?: Record<string, unknown>;
}
