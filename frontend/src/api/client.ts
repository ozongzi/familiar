// ─── Core fetch wrapper ───────────────────────────────────────────────────────

const BASE = "";

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
  token?: string | null
): Promise<T> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const res = await fetch(`${BASE}${path}`, {
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
} from "./types";

export const api = {
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

  renameConversation(token: string, id: string, body: RenameConversationRequest) {
    return patch<Conversation>(`/api/conversations/${id}`, body, token);
  },

  // ── Messages ─────────────────────────────────────────────────────────────
  listMessages(token: string, conversationId: string) {
    return get<Message[]>(`/api/conversations/${conversationId}/messages`, token);
  },
};
