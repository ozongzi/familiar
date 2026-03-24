import { useState, useEffect } from "react";
import { useAuth } from "./store/auth.shared";
import { LoginPage } from "./pages/LoginPage";
import { ChatPage } from "./pages/ChatPage";
import { AdminPage } from "./pages/AdminPage";
import { PrivacyConsentModal } from "./components/PrivacyConsentModal";
import { PrivacyPage } from "./pages/PrivacyPage";
import { Routes, Route, Navigate } from "react-router-dom";

export function App() {
  const { token, loading, user, logout } = useAuth();
  const [showPrivacy, setShowPrivacy] = useState(false);

  useEffect(() => {
    if (token && sessionStorage.getItem("familiar_show_privacy") === "1") {
      setShowPrivacy(true);
    }
  }, [token]);

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

  if (showPrivacy) {
    return (
      <PrivacyConsentModal
        onAccept={() => {
          sessionStorage.removeItem("familiar_show_privacy");
          setShowPrivacy(false);
        }}
        onDecline={() => {
          sessionStorage.removeItem("familiar_show_privacy");
          logout();
        }}
      />
    );
  }

  return (
    <Routes>
      <Route path="/" element={<ChatPage />} />
      <Route path="/:conversationId" element={<ChatPage />} />
      <Route
        path="/admin"
        element={user?.is_admin ? <AdminPage /> : <Navigate to="/" replace />}
      />
      <Route path="/privacy" element={<PrivacyPage />} />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
