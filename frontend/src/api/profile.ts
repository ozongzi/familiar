import type {
  MeResponse,
  UpdateProfileRequest,
  UpdatePasswordRequest,
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

function put<T>(path: string, body: unknown, token?: string | null) {
  return request<T>("PUT", path, body, token);
}

async function upload<T>(
  path: string,
  formData: FormData,
  token?: string | null,
): Promise<T> {
  const headers: Record<string, string> = {};
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const res = await fetch(`${BASE}${path}`, {
    method: "POST",
    headers,
    body: formData,
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

  return res.json() as Promise<T>;
}

// ─── Profile ─────────────────────────────────────────────────────────────────

export function getProfile(token: string): Promise<MeResponse> {
  return get<MeResponse>("/api/users/me", token);
}

export function updateProfile(
  data: UpdateProfileRequest,
  token: string,
): Promise<MeResponse> {
  return put<MeResponse>("/api/users/me/profile", data, token);
}

export function updatePassword(
  data: UpdatePasswordRequest,
  token: string,
): Promise<{ message: string }> {
  return put<{ message: string }>("/api/users/me/password", data, token);
}

export function uploadAvatar(
  file: File,
  token: string,
): Promise<{ avatar_path: string; message: string }> {
  const formData = new FormData();
  formData.append("avatar", file);
  return upload<{ avatar_path: string; message: string }>(
    "/api/users/me/avatar",
    formData,
    token,
  );
}

export function getAvatarUrl(userId: string): string {
  return `${BASE}/api/avatars/${userId}`;
}
