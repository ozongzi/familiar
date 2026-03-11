import { useEffect, useReducer, type ReactNode } from "react";
import { api } from "../api/client";
import { authReducer, AuthContext, TOKEN_KEY } from "./auth.shared";

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
