// ─── Core fetch wrapper ───────────────────────────────────────────────────────
import { getServerBase } from "../utils/tauri";

const BASE = () => getServerBase();

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
  token?: string | null,
): Promise<T> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const res = await fetch(`${BASE()}${path}`, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });

  if (!res.ok) {
    let message = `HTTP ${res.status}`;
    try {
      const err = await res.json();
      if (err?.error) message = err.error;
    } catch {
      // ignore parse failure
    }
    throw new Error(message);
  }

  // 204 No Content
  if (res.status === 204) return undefined as unknown as T;

  return res.json() as Promise<T>;
}

function get<T>(path: string, token?: string | null) {
  return request<T>("GET", path, undefined, token);
}

function post<T>(path: string, body: unknown, token?: string | null) {
  return request<T>("POST", path, body, token);
}

function patch<T>(path: string, body: unknown, token?: string | null) {
  return request<T>("PATCH", path, body, token);
}

function del<T>(path: string, token?: string | null) {
  return request<T>("DELETE", path, undefined, token);
}

// ─── Auth ─────────────────────────────────────────────────────────────────────

import type {
  LoginRequest,
  LoginResponse,
  RegisterRequest,
  RegisterResponse,
  MeResponse,
  Conversation,
  CreateConversationRequest,
  RenameConversationRequest,
  Message,
  Mcp,
  CreateMcpRequest,
  UserSettings,
  UpdateSettingsRequest,
  Skill,
  CreateSkillRequest,
  AdminConfig,
  AppSkill,
  SearchResult,
  Model,
  UpsertModelRequest,
  ModelPermissionsResponse,
  UpdateModelPermissionsRequest,
} from "./types";

export const api = {
  // ── Settings ────────────────────────────────────────────────────────────
  getSettings(token: string) {
    return get<UserSettings>("/api/settings", token);
  },

  updateSettings(token: string, body: UpdateSettingsRequest) {
    return post<{ ok: boolean }>("/api/settings", body, token);
  },

  getTokenUsage(token: string) {
    return get<{ prompt_tokens: number; completion_tokens: number; cache_read_tokens: number; cache_creation_tokens: number; total_tokens: number; conversation_count: number }>("/api/admin/token-usage", token);
  },

  getTokenUsageByUser(token: string) {
    return get<{ users: { user_id: string; username: string; conversation_count: number; prompt_tokens: number; completion_tokens: number; cache_read_tokens: number; cache_creation_tokens: number; total_tokens: number }[] }>("/api/admin/token-usage/by-user", token);
  },

  getTokenUsageConversations(token: string, userId?: string) {
    const q = userId ? `?user_id=${userId}` : "";
    return get<{ conversations: { conv_id: string; conv_name: string; username: string; created_at: string; prompt_tokens: number; completion_tokens: number; cache_read_tokens: number; cache_creation_tokens: number; total_tokens: number }[] }>(`/api/admin/token-usage/conversations${q}`, token);
  },

  getTokenUsageDaily(token: string) {
    return get<{ days: { day: string; total_tokens: number; prompt_tokens: number; completion_tokens: number; cache_read_tokens: number; cache_creation_tokens: number; conversation_count: number }[] }>("/api/admin/token-usage/daily", token);
  },

  getAdminConfig(token: string) {
    return get<AdminConfig>("/api/admin/config", token);
  },

  updateAdminConfig(token: string, body: AdminConfig) {
    return post<{ ok: boolean }>("/api/admin/config", body, token);
  },

  listAdminSkills(token: string) {
    return get<AppSkill[]>("/api/admin/skills", token);
  },

  createAdminSkill(token: string, body: CreateSkillRequest) {
    return post<AppSkill>("/api/admin/skills", body, token);
  },

  updateAdminSkill(token: string, id: string, body: CreateSkillRequest) {
    return request<AppSkill>("PUT", `/api/admin/skills/${id}`, body, token);
  },

  deleteAdminSkill(token: string, id: string) {
    return del<{ ok: boolean }>(`/api/admin/skills/${id}`, token);
  },

  // ── Skills ──────────────────────────────────────────────────────────────
  // Frontend API methods for user skills.
  listSkills(token: string) {
    return get<Skill[]>("/api/skills", token);
  },

  createSkill(token: string, body: CreateSkillRequest) {
    return post<Skill>("/api/skills", body, token);
  },

  updateSkill(token: string, id: string, body: CreateSkillRequest) {
    return request<Skill>("PUT", `/api/skills/${id}`, body, token);
  },

  deleteSkill(token: string, id: string) {
    return del<{ ok: boolean }>(`/api/skills/${id}`, token);
  },

  // ── Sessions ────────────────────────────────────────────────────────────
  login(body: LoginRequest) {
    return post<LoginResponse>("/api/sessions", body);
  },

  logout(token: string) {
    return del<{ ok: boolean }>("/api/sessions", token);
  },

  // ── Users ───────────────────────────────────────────────────────────────
  register(body: RegisterRequest) {
    return post<RegisterResponse>("/api/users", body);
  },

  getMe(token: string) {
    return get<MeResponse>("/api/users/me", token);
  },

  // ── Conversations ────────────────────────────────────────────────────────
  listConversations(token: string) {
    return get<Conversation[]>("/api/conversations", token);
  },

  createConversation(token: string, body: CreateConversationRequest = {}) {
    return post<Conversation>("/api/conversations", body, token);
  },

  deleteConversation(token: string, id: string) {
    return del<{ ok: boolean }>(`/api/conversations/${id}`, token);
  },

  renameConversation(
    token: string,
    id: string,
    body: RenameConversationRequest,
  ) {
    return patch<Conversation>(`/api/conversations/${id}`, body, token);
  },

  // ── Auto-title ────────────────────────────────────────────────────────────
  autoTitle(token: string, conversationId: string, prompt: string) {
    return post<{ title: string }>(
      `/api/conversations/${conversationId}/title`,
      { prompt },
      token,
    );
  },

  listMessages(token: string, conversationId: string) {
    return get<Message[]>(
      `/api/conversations/${conversationId}/messages`,
      token,
    );
  },

  activateMessage(
    token: string,
    conversationId: string,
    messageId: number,
  ) {
    return post<{ active_message_id: number }>(
      `/api/conversations/${conversationId}/activate`,
      { message_id: messageId },
      token,
    );
  },

  searchMessages(token: string, q: string, limit = 20) {
    const params = new URLSearchParams({ q, limit: String(limit) });
    return get<{ results: SearchResult[] }>(`/api/search?${params}`, token);
  },

  sendMessage(token: string, conversationId: string, content: string) {
    return post<{ stream_id: string }>(
      `/api/conversations/${conversationId}/messages`,
      { content },
      token,
    );
  },

  // ── Models ──────────────────────────────────────────────────
  listModels(token: string) {
    return get<Model[]>("/api/models", token);
  },

  createModel(token: string, body: UpsertModelRequest) {
    return post<Model>("/api/models", body, token);
  },

  updateModel(token: string, id: string, body: UpsertModelRequest) {
    return request<Model>("PUT", `/api/models/${id}`, body, token);
  },

  deleteModel(token: string, id: string) {
    return del<{ ok: boolean }>(`/api/models/${id}`, token);
  },

  adminListModels(token: string) {
    return get<Model[]>("/api/admin/models", token);
  },

  adminCreateModel(token: string, body: UpsertModelRequest) {
    return post<Model>("/api/admin/models", body, token);
  },

  adminUpdateModel(token: string, id: string, body: UpsertModelRequest) {
    return request<Model>("PUT", `/api/admin/models/${id}`, body, token);
  },

  adminDeleteModel(token: string, id: string) {
    return del<{ ok: boolean }>(`/api/admin/models/${id}`, token);
  },

  getModelPermissions(token: string) {
    return get<ModelPermissionsResponse>("/api/admin/model-permissions", token);
  },

  updateModelPermissions(token: string, body: UpdateModelPermissionsRequest) {
    return request<{ ok: boolean }>("PUT", "/api/admin/model-permissions", body, token);
  },

  // ── MCPs ─────────────────────────────────────────────────────────────────
  listMcps(token: string) {
    return get<Mcp[]>("/api/mcps", token);
  },

  createMcp(token: string, body: CreateMcpRequest) {
    return post<Mcp>("/api/mcps", body, token);
  },

  updateMcp(token: string, id: string, body: CreateMcpRequest) {
    return request<Mcp>("PUT", `/api/mcps/${id}`, body, token);
  },

  deleteMcp(token: string, id: string) {
    return del<{ ok: boolean }>(`/api/mcps/${id}`, token);
  },
};
