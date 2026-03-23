import { useState, useEffect } from "react";
import styles from "./AdminPage.module.css";
import { UserManagement } from "../components/UserManagement";
import { AuditLogView } from "../components/AuditLogView";
import { AdminConfig } from "../components/AdminConfig";
import { useNavigate } from "react-router-dom";
import { useAuth } from "../store/auth.shared";
import { api } from "../api/client";

type AdminView = "users" | "audit" | "config" | "usage";

type TokenUsage = { prompt_tokens: number; completion_tokens: number; total_tokens: number; conversation_count: number };

export function AdminPage() {
  const [currentView, setCurrentView] = useState<AdminView>("users");
  const [usage, setUsage] = useState<TokenUsage | null>(null);
  const { token } = useAuth();
  const navigate = useNavigate();

  useEffect(() => {
    if (currentView !== "usage" || !token) return;
    api.getTokenUsage(token).then(setUsage).catch(() => {});
  }, [currentView, token]);

  const navigateToChat = () => {
    navigate("/");
  };

  return (
    <div className={styles.container}>
      {/* Sidebar */}
      <aside className={styles.sidebar}>
        <div className={styles.sidebarHeader}>
          <h2>管理面板</h2>
        </div>
        <nav className={styles.nav}>
          <button
            className={`${styles.navItem} ${currentView === "users" ? styles.navItemActive : ""}`}
            onClick={() => setCurrentView("users")}
          >
            <span className={styles.navIcon}>👥</span>
            用户管理
          </button>
          <button
            className={`${styles.navItem} ${currentView === "audit" ? styles.navItemActive : ""}`}
            onClick={() => setCurrentView("audit")}
          >
            <span className={styles.navIcon}>📋</span>
            审计日志
          </button>
          <button
            className={`${styles.navItem} ${currentView === "config" ? styles.navItemActive : ""}`}
            onClick={() => setCurrentView("config")}
          >
            <span className={styles.navIcon}>⚙️</span>
            系统配置
          </button>
          <button
            className={`${styles.navItem} ${currentView === "usage" ? styles.navItemActive : ""}`}
            onClick={() => setCurrentView("usage")}
          >
            <span className={styles.navIcon}>📊</span>
            Token 用量
          </button>
        </nav>
        <div className={styles.sidebarFooter}>
          <button className={styles.backBtn} onClick={navigateToChat}>
            ← 返回聊天
          </button>
        </div>
      </aside>

      {/* Main content */}
      <main className={styles.content}>
        <div className={styles.contentHeader}>
          <h1>
            {currentView === "users" && "用户管理"}
            {currentView === "audit" && "审计日志"}
            {currentView === "config" && "系统配置"}
            {currentView === "usage" && "Token 用量"}
          </h1>
        </div>
        <div className={styles.contentBody}>
          {currentView === "users" && <UserManagement />}
          {currentView === "audit" && <AuditLogView />}
          {currentView === "config" && <AdminConfig />}
          {currentView === "usage" && (
            <div style={{ display: "flex", flexDirection: "column", gap: 16, maxWidth: 480 }}>
              {usage ? (
                ["conversation_count", "total_tokens", "prompt_tokens", "completion_tokens"].map(k => (
                  <div key={k} style={{ display: "flex", justifyContent: "space-between", padding: "12px 16px", background: "var(--bg-surface)", border: "1px solid var(--border)", borderRadius: "var(--radius-md)" }}>
                    <span style={{ color: "var(--text-secondary)", fontSize: "0.9em" }}>{{ conversation_count: "对话数", total_tokens: "总 Token", prompt_tokens: "Prompt Token", completion_tokens: "Completion Token" }[k]}</span>
                    <strong>{(usage as Record<string,number>)[k].toLocaleString()}</strong>
                  </div>
                ))
              ) : (
                <p style={{ color: "var(--text-muted)" }}>加载中…</p>
              )}
            </div>
          )}
        </div>
      </main>
    </div>
  );
}
