import { createContext, useContext } from "react";
import type { MeResponse } from "../api/types";

/**
 * Shared auth types, reducer and context used by the AuthProvider implementation.
 * This file contains only non-component logic (and the `useAuth` hook) so that
 * the component file (`auth.tsx`) can export the provider component without
 * triggering the react-refresh rule that requires component-only exports.
 */

/* ── Types ─────────────────────────────────────────────────────────────────── */

export interface AuthState {
  token: string | null;
  user: MeResponse | null;
  loading: boolean;
}

export type AuthAction =
  | { type: "SET_TOKEN"; token: string }
  | { type: "SET_USER"; user: MeResponse }
  | { type: "LOGOUT" }
  | { type: "SET_LOADING"; loading: boolean };

export interface AuthContextValue {
  token: string | null;
  user: MeResponse | null;
  loading: boolean;
  login: (token: string) => Promise<void>;
  logout: () => Promise<void>;
}

/* ── Reducer ───────────────────────────────────────────────────────────────── */

export function authReducer(state: AuthState, action: AuthAction): AuthState {
  switch (action.type) {
    case "SET_TOKEN":
      return { ...state, token: action.token };
    case "SET_USER":
      return { ...state, user: action.user, loading: false };
    case "LOGOUT":
      return { token: null, user: null, loading: false };
    case "SET_LOADING":
      return { ...state, loading: action.loading };
    default:
      return state;
  }
}

/* ── Context / Hook ────────────────────────────────────────────────────────── */

/**
 * AuthContext - created here and consumed by `useAuth`.
 * The actual AuthProvider component (which performs side effects and dispatches
 * actions) should live in `auth.tsx` and import this `AuthContext` and the
 * `authReducer` / `TOKEN_KEY` from this file.
 */
export const AuthContext = createContext<AuthContextValue | null>(null);

/**
 * Key for storing the token in localStorage.
 */
export const TOKEN_KEY = "familiar_token";

/**
 * useAuth - hook that consumers call to access auth context.
 * Throws if used outside of an AuthProvider.
 */
export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) {
    throw new Error("useAuth must be used inside <AuthProvider>");
  }
  return ctx;
}
