import { useState } from "react";
import styles from "./AdminPage.module.css";
import { UserManagement } from "../components/UserManagement";
import { AuditLogView } from "../components/AuditLogView";
import { AdminConfig } from "../components/AdminConfig";
import { AppSkillsPanel } from "../components/SkillsPanel";
import { AdminModelsPanel } from "../components/AdminModelsPanel";
import { AdminOverview } from "../components/AdminOverview";
import { InviteCodesPanel } from "../components/InviteCodesPanel";
import { SqlPanel } from "../components/SqlPanel";
import { useNavigate } from "react-router-dom";
import { useAuth } from "../store/auth.shared";

export type AdminView =
  | "overview"
  | "config"
  | "models"
  | "users"
  | "invites"
  | "skills"
  | "audit"
  | "sql";

const NAV: { id: AdminView; label: string }[] = [
  { id: "overview", label: "概览" },
  { id: "models",   label: "全局模型" },
  { id: "config",   label: "系统配置" },
  { id: "users",    label: "用户管理" },
  { id: "invites",  label: "邀请码" },
  { id: "skills",   label: "默认技能" },
  { id: "audit",    label: "审计日志" },
  { id: "sql",      label: "SQL" },
];

export function AdminPage() {
  const [view, setView] = useState<AdminView>("overview");
  const navigate = useNavigate();
  const { token } = useAuth();

  return (
    <div className={styles.container}>
      <header className={styles.header}>
        <button className={styles.backBtn} onClick={() => navigate("/")}>
          <ChevronLeftIcon />
          返回
        </button>
        <nav className={styles.nav}>
          {NAV.map(({ id, label }) => (
            <button
              key={id}
              className={`${styles.navBtn} ${view === id ? styles.navBtnActive : ""}`}
              onClick={() => setView(id)}
            >
              {label}
            </button>
          ))}
        </nav>
      </header>

      <main className={styles.main}>
        {view === "overview" && <AdminOverview token={token ?? ""} onNavigate={setView} />}
        {view === "models"   && <AdminModelsPanel token={token ?? ""} />}
        {view === "config"   && <AdminConfig />}
        {view === "users"    && <UserManagement />}
        {view === "invites"  && <InviteCodesPanel />}
        {view === "skills"   && <AppSkillsPanel token={token ?? ""} />}
        {view === "audit"    && <AuditLogView />}
        {view === "sql"      && <SqlPanel />}
      </main>
    </div>
  );
}

function ChevronLeftIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <polyline points="15 18 9 12 15 6" />
    </svg>
  );
}
