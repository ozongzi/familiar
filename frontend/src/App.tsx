import { useState, useEffect } from "react";
import { useAuth } from "./store/auth.shared";
import { getServerBase } from "./utils/tauri";
import { LoginPage } from "./pages/LoginPage";
import { ChatPage } from "./pages/ChatPage";
import { AdminPage } from "./pages/AdminPage";
import { PrivacyConsentModal } from "./components/PrivacyConsentModal";
import { PrivacyPage } from "./pages/PrivacyPage";
import {
  SharedConversationPage,
  consumePendingShareImport,
} from "./pages/SharedConversationPage";
import { Toaster } from "./components/Toaster";
import { Routes, Route, Navigate, useLocation } from "react-router-dom";
import { api } from "./api/client";
import { checkForAppUpdate } from "./utils/autoUpdate";

export function App() {
  const { token, loading, user, logout, login } = useAuth();
  const location = useLocation();
  const [showPrivacy, setShowPrivacy] = useState(false);

  // Public routes that don't require auth.
  const isPublicRoute =
    location.pathname.startsWith("/share/") ||
    location.pathname === "/privacy";

  useEffect(() => {
    if (token && sessionStorage.getItem("familiar_show_privacy") === "1") {
      setShowPrivacy(true);
    }
  }, [token, setShowPrivacy]);

  // One-shot updater check on boot (Tauri only — silent on web).
  useEffect(() => {
    void checkForAppUpdate();
  }, []);

  // After login, if a share-import was deferred, resolve it now.
  useEffect(() => {
    if (!token) return;
    const pending = consumePendingShareImport();
    if (!pending) return;
    api
      .importSharedConversation(token, pending)
      .then((res) => {
        window.location.replace(`/${res.conversation_id}`);
      })
      .catch(() => {
        /* swallow — user can retry from the share page */
      });
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

  if (!token && !isPublicRoute) {
    return (
      <LoginPage
        serverUrl={getServerBase()}
        onLogin={async (t) => { await login(t); }}
      />
    );
  }

  if (token && showPrivacy) {
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
    <>
    <Toaster />
    <Routes>
      <Route path="/share/:shareToken" element={<SharedConversationPage />} />
      <Route path="/privacy" element={<PrivacyPage />} />
      <Route path="/" element={<ChatPage />} />
      <Route path="/:conversationId" element={<ChatPage />} />
      <Route
        path="/admin"
        element={user?.is_admin ? <AdminPage /> : <Navigate to="/" replace />}
      />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
    </>
  );
}
