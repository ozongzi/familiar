import { useAuth } from "./store/auth.shared";
import { LoginPage } from "./pages/LoginPage";
import { ChatPage } from "./pages/ChatPage";
import { AdminPage } from "./pages/AdminPage";
import { useState, useEffect } from "react";

export function App() {
  const { token, loading, user } = useAuth();
  const [currentPath, setCurrentPath] = useState(window.location.pathname);

  useEffect(() => {
    const handlePopState = () => setCurrentPath(window.location.pathname);
    window.addEventListener("popstate", handlePopState);
    return () => window.removeEventListener("popstate", handlePopState);
  }, []);

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

  if (!token) {
    return <LoginPage />;
  }

  // Route guard: only admins can access /admin
  if (currentPath.startsWith("/admin")) {
    if (!user?.is_admin) {
      window.history.pushState({}, "", "/");
      setCurrentPath("/");
      return <ChatPage initialConversationId={null} />;
    }
    return <AdminPage />;
  }

  // Extract conversation ID from path
  // Path format: "/" (draft) or "/conversation-id" (existing conversation)
  const pathSegments = currentPath.split("/").filter(Boolean);
  const conversationId = pathSegments.length > 0 ? pathSegments[0] : null;

  return <ChatPage initialConversationId={conversationId} />;
}
