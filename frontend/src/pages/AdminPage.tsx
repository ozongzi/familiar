import React, { useState } from "react";
import styles from "./AdminPage.module.css";
import { UserManagement } from "../components/UserManagement";
import { AuditLogView } from "../components/AuditLogView";
import { AdminConfig } from "../components/AdminConfig";
import { TokenUsageView } from "../components/TokenUsageView";
import { AppSkillsPanel } from "../components/SkillsPanel";
import { AdminModelsPanel } from "../components/AdminModelsPanel";
import { useNavigate } from "react-router-dom";
import { useAuth } from "../store/auth.shared";

type AdminView = "users" | "audit" | "config" | "usage" | "skills" | "models";

const TABS: { id: AdminView; label: string; icon: () => React.ReactElement }[] = [
  { id: "users",  label: "用户管理", icon: UsersIcon },
  { id: "audit",  label: "审计日志", icon: AuditIcon },
  { id: "config", label: "系统配置", icon: ConfigIcon },
  { id: "usage",  label: "Token 用量", icon: UsageIcon },
  { id: "skills", label: "默认 Skills", icon: SkillsIcon },
  { id: "models", label: "全局模型", icon: ModelsIcon },
];

export function AdminPage() {
  const [currentView, setCurrentView] = useState<AdminView>("users");
  const navigate = useNavigate();
  const { token } = useAuth();

  return (
    <div className={styles.container}>
      {/* Top tab bar */}
      <div className={styles.tabBar}>
        <div className={styles.tabList}>
          {TABS.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              className={`${styles.tab} ${currentView === id ? styles.tabActive : ""}`}
              onClick={() => setCurrentView(id)}
            >
              <Icon />
              {label}
            </button>
          ))}
        </div>
        <button className={styles.backBtn} onClick={() => navigate("/")}>
          <ChevronLeftIcon />
          返回聊天
        </button>
      </div>

      {/* Content */}
      <div className={styles.content}>
        {currentView === "users"  && <UserManagement />}
        {currentView === "audit"  && <AuditLogView />}
        {currentView === "config" && <AdminConfig />}
        {currentView === "usage"  && <TokenUsageView />}
        {currentView === "skills" && <AppSkillsPanel token={token ?? ""} />}
        {currentView === "models" && <AdminModelsPanel token={token ?? ""} />}
      </div>
    </div>
  );
}

function UsersIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
      <circle cx="9" cy="7" r="4" />
      <path d="M23 21v-2a4 4 0 0 0-3-3.87" />
      <path d="M16 3.13a4 4 0 0 1 0 7.75" />
    </svg>
  );
}

function AuditIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <polyline points="14 2 14 8 20 8" />
      <line x1="16" y1="13" x2="8" y2="13" />
      <line x1="16" y1="17" x2="8" y2="17" />
      <line x1="10" y1="9" x2="8" y2="9" />
    </svg>
  );
}

function ConfigIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}

function UsageIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <line x1="18" y1="20" x2="18" y2="10" />
      <line x1="12" y1="20" x2="12" y2="4" />
      <line x1="6" y1="20" x2="6" y2="14" />
    </svg>
  );
}

function SkillsIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 2a7 7 0 0 1 7 7c0 3.87-3.13 7-7 7S5 12.87 5 9a7 7 0 0 1 7-7z" />
      <path d="M12 16v6" />
      <path d="M8 22h8" />
    </svg>
  );
}

function ModelsIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <ellipse cx="12" cy="5" rx="9" ry="3" />
      <path d="M21 12c0 1.66-4.03 3-9 3S3 13.66 3 12" />
      <path d="M3 5v14c0 1.66 4.03 3 9 3s9-1.34 9-3V5" />
    </svg>
  );
}

function ChevronLeftIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <polyline points="15 18 9 12 15 6" />
    </svg>
  );
}
