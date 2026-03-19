import { useEffect, useReducer, type ReactNode } from "react";
import { api } from "../api/client";
import { authReducer, AuthContext, TOKEN_KEY } from "./auth.shared";
import { invoke, isTauri, getServerBase } from "../utils/tauri";

// Provider implementation uses shared reducer/context/constants from auth.shared
export function AuthProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(authReducer, {
    token:
      typeof window !== "undefined" ? localStorage.getItem(TOKEN_KEY) : null,
    user: null,
    loading: true,
  });

  // On mount (or token change), fetch /api/users/me to validate the token. 
  useEffect(() => {
    if (!state.token) {
      dispatch({ type: "SET_LOADING", loading: false });
      return;
    }

    let cancelled = false;
    dispatch({ type: "SET_LOADING", loading: true });

    api
      .getMe(state.token)
      .then((user) => {
        if (!cancelled) dispatch({ type: "SET_USER", user });
      })
      .catch(() => {
        if (!cancelled) {
          localStorage.removeItem(TOKEN_KEY);
          dispatch({ type: "LOGOUT" });
        }
      });

    return () => {
      cancelled = true;
    };
  }, [state.token]);

  // 桌面端：token 变化时启动/停止 WS 隧道
  useEffect(() => {
    if (!isTauri()) return;

    if (state.token) {
      const serverUrl = getServerBase();
      invoke("start_tunnel", { token: state.token, serverUrl }).catch((e) =>
        console.warn("[tunnel] start failed:", e),
      );
    } else {
      invoke("stop_tunnel").catch((e) =>
        console.warn("[tunnel] stop failed:", e),
      );
    }
  }, [state.token]);

  async function login(token: string) {
    localStorage.setItem(TOKEN_KEY, token);
    dispatch({ type: "SET_TOKEN", token });
    // SET_USER will be dispatched by the useEffect above.
  }

  async function logout() {
    if (state.token) {
      try {
        await api.logout(state.token);
      } catch {
        // best-effort
      }
    }
    localStorage.removeItem(TOKEN_KEY);
    dispatch({ type: "LOGOUT" });
  }

  return (
    <AuthContext.Provider
      value={{
        token: state.token,
        user: state.user,
        loading: state.loading,
        login,
        logout,
      }}
    >
      {children}
    </AuthContext.Provider>
  );
}
