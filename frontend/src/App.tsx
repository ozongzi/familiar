import { useAuth } from "./store/auth.shared";
import { LoginPage } from "./pages/LoginPage";
import { ChatPage } from "./pages/ChatPage";

export function App() {
  const { token, loading } = useAuth();

  if (loading) {
    return (
      <div
        style={{
          height: "100%",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "var(--bg-base)",
          color: "var(--text-muted)",
          fontSize: "0.9em",
          gap: "10px",
        }}
      >
        <img src="/favicon.svg" width={24} height={24} alt="" />
        <span>加载中…</span>
      </div>
    );
  }

  return token ? <ChatPage /> : <LoginPage />;
}
