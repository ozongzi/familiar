import type {
  User,
  UsersPage,
  CreateUserRequest,
  UpdateUserRequest,
  AuditLogPage,
  AuditLogQuery,
  GlobalMcp,
  CreateGlobalMcpRequest,
  UpdateGlobalMcpRequest,
} from "./types";

const BASE = "";

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

  if (res.status === 204) return undefined as unknown as T;

  return res.json() as Promise<T>;
}

function get<T>(path: string, token?: string | null) {
  return request<T>("GET", path, undefined, token);
}

function post<T>(path: string, body: unknown, token?: string | null) {
  return request<T>("POST", path, body, token);
}

function put<T>(path: string, body: unknown, token?: string | null) {
  return request<T>("PUT", path, body, token);
}

function del<T>(path: string, token?: string | null) {
  return request<T>("DELETE", path, undefined, token);
}

// ─── User Management ─────────────────────────────────────────────────────────

export function listUsers(
  params: {
    page?: number;
    per_page?: number;
    search?: string;
  },
  token: string,
): Promise<UsersPage> {
  const query = new URLSearchParams();
  if (params.page) query.set("page", params.page.toString());
  if (params.per_page) query.set("per_page", params.per_page.toString());
  if (params.search) query.set("search", params.search);

  const queryString = query.toString();
  const path = `/api/admin/users${queryString ? `?${queryString}` : ""}`;

  return get<UsersPage>(path, token);
}

export function createUser(
  data: CreateUserRequest,
  token: string,
): Promise<User> {
  return post<User>("/api/admin/users", data, token);
}

export function updateUser(
  id: string,
  data: UpdateUserRequest,
  token: string,
): Promise<User> {
  return put<User>(`/api/admin/users/${id}`, data, token);
}

export function deleteUser(id: string, token: string): Promise<{ ok: boolean }> {
  return del<{ ok: boolean }>(`/api/admin/users/${id}`, token);
}

export function resetPassword(
  id: string,
  newPassword: string,
  token: string,
): Promise<{ message: string }> {
  return post<{ message: string }>(
    `/api/admin/users/${id}/reset-password`,
    { new_password: newPassword },
    token,
  );
}

// ─── Audit Logs ──────────────────────────────────────────────────────────────

export function listAuditLogs(
  params: AuditLogQuery,
  token: string,
): Promise<AuditLogPage> {
  const query = new URLSearchParams();
  if (params.page) query.set("page", params.page.toString());
  if (params.per_page) query.set("per_page", params.per_page.toString());
  if (params.user_id) query.set("user_id", params.user_id);
  if (params.target_user_id) query.set("target_user_id", params.target_user_id);
  if (params.action) query.set("action", params.action);
  if (params.start_date) query.set("start_date", params.start_date);
  if (params.end_date) query.set("end_date", params.end_date);

  const queryString = query.toString();
  const path = `/api/admin/audit-logs${queryString ? `?${queryString}` : ""}`;

  return get<AuditLogPage>(path, token);
}

// ─── Global MCPs ─────────────────────────────────────────────────────────────

export function listGlobalMcps(token: string): Promise<GlobalMcp[]> {
  return get<GlobalMcp[]>("/api/admin/mcps", token);
}

export function createGlobalMcp(
  data: CreateGlobalMcpRequest,
  token: string,
): Promise<GlobalMcp> {
  return post<GlobalMcp>("/api/admin/mcps", data, token);
}

export function updateGlobalMcp(
  id: string,
  data: UpdateGlobalMcpRequest,
  token: string,
): Promise<GlobalMcp> {
  return put<GlobalMcp>(`/api/admin/mcps/${id}`, data, token);
}

export function deleteGlobalMcp(id: string, token: string): Promise<{ ok: boolean }> {
  return del<{ ok: boolean }>(`/api/admin/mcps/${id}`, token);
}
