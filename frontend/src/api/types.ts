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
  reasoning: string | null;
  is_summary: boolean;
  created_at: number;
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
      source?: "spawn";
    }
  | { type: "user_interrupt"; content: string }
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
  _rawArgs?: string;
}

export interface UploadBubble {
  kind: "upload";
  key: string;
  role: "user";
  filename: string;
  path: string;
  size: number;
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
  system_prompt: string | null;
}

export interface ModelConfig {
  name: string;
  api_key: string;
  api_base: string;
  extra_body: Record<string, unknown>;
}

export interface UpdateSettingsRequest {
  mode: "custom" | "default";
  api_key?: string | null;
  api_base?: string | null;
  model_name?: string | null;
  system_prompt?: string | null;
}

export interface ServerConfig {
  port: number;
  system_prompt: string | null;
  subagent_prompt: string | null;
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

export interface AdminConfig {
  public_path: string;
  artifacts_path: string;
  frontier_model: ModelConfig;
  cheap_model: ModelConfig;
  embedding: ModelConfig;
  server: ServerConfig;
  mcp: McpServerConfig[];
  mcp_catalog: McpCatalogEntry[];
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
