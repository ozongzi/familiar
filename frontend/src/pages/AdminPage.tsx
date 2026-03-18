import { useState } from "react";
import styles from "./AdminPage.module.css";
import { UserManagement } from "../components/UserManagement";
import { AuditLogView } from "../components/AuditLogView";
import { AdminConfig } from "../components/AdminConfig";

type AdminView = "users" | "audit" | "config";

export function AdminPage() {
  const [currentView, setCurrentView] = useState<AdminView>("users");

  const navigateToChat = () => {
    window.history.pushState({}, "", "/");
    window.dispatchEvent(new PopStateEvent("popstate"));
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
          </h1>
        </div>
        <div className={styles.contentBody}>
          {currentView === "users" && <UserManagement />}
          {currentView === "audit" && <AuditLogView />}
          {currentView === "config" && <AdminConfig />}
        </div>
      </main>
    </div>
  );
}
