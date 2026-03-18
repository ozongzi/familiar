import { useState, useEffect } from "react";
import styles from "./AuditLogView.module.css";
import { useAuth } from "../store/auth.shared";
import { listAuditLogs } from "../api/admin";
import type { AuditLog } from "../api/types";

export function AuditLogView() {
  const { token } = useAuth();
  const [logs, setLogs] = useState<AuditLog[]>([]);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(1);
  const [perPage] = useState(50);
  const [action, setAction] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());

  const loadLogs = async () => {
    if (!token) return;
    setLoading(true);
    setError(null);
    try {
      const result = await listAuditLogs(
        {
          page,
          per_page: perPage,
          action: action || undefined,
        },
        token
      );
      setLogs(result.items);
      setTotal(result.total);
    } catch (err) {
      setError(err instanceof Error ? err.message : "加载日志失败");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadLogs();
  }, [page, action]);

  const toggleExpand = (id: string) => {
    const newSet = new Set(expandedIds);
    if (newSet.has(id)) {
      newSet.delete(id);
    } else {
      newSet.add(id);
    }
    setExpandedIds(newSet);
  };

  const formatTime = (dateStr: string) => {
    const date = new Date(dateStr);
    const now = new Date();
    const diff = now.getTime() - date.getTime();
    const minutes = Math.floor(diff / 60000);

    if (minutes < 1) return "刚刚";
    if (minutes < 60) return `${minutes}分钟前`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours}小时前`;
    const days = Math.floor(hours / 24);
    if (days < 7) return `${days}天前`;

    return date.toLocaleString("zh-CN");
  };

  const getActionLabel = (action: string) => {
    const labels: Record<string, string> = {
      login: "登录",
      register: "注册",
      create_user: "创建用户",
      update_user: "更新用户",
      delete_user: "删除用户",
      reset_password: "重置密码",
      update_profile: "更新个人资料",
      update_password: "修改密码",
      upload_avatar: "上传头像",
    };
    return labels[action] || action;
  };

  const getActionColor = (action: string) => {
    if (action.includes("delete")) return "danger";
    if (action.includes("create") || action === "register") return "success";
    if (action.includes("update") || action.includes("reset")) return "warning";
    return "info";
  };

  const totalPages = Math.ceil(total / perPage);

  return (
    <div className={styles.container}>
      {/* Filters */}
      <div className={styles.filters}>
        <div className={styles.filterGroup}>
          <label>操作类型</label>
          <select
            value={action}
            onChange={(e) => {
              setAction(e.target.value);
              setPage(1);
            }}
            className={styles.select}
          >
            <option value="">全部</option>
            <option value="login">登录</option>
            <option value="register">注册</option>
            <option value="create_user">创建用户</option>
            <option value="update_user">更新用户</option>
            <option value="delete_user">删除用户</option>
            <option value="reset_password">重置密码</option>
            <option value="update_profile">更新资料</option>
            <option value="update_password">修改密码</option>
          </select>
        </div>
      </div>

      {/* Error */}
      {error && <div className={styles.error}>{error}</div>}

      {/* Table */}
      {loading ? (
        <div className={styles.loading}>加载中...</div>
      ) : logs.length === 0 ? (
        <div className={styles.empty}>没有日志记录</div>
      ) : (
        <>
          <div className={styles.tableWrapper}>
            <table className={styles.table}>
              <thead className={styles.thead}>
                <tr>
                  <th style={{ width: "140px" }}>时间</th>
                  <th style={{ width: "120px" }}>操作类型</th>
                  <th>操作者</th>
                  <th>目标用户</th>
                  <th>IP地址</th>
                  <th style={{ width: "80px" }}>详情</th>
                </tr>
              </thead>
              <tbody className={styles.tbody}>
                {logs.map((log) => (
                  <>
                    <tr key={log.id} className={styles.row}>
                      <td className={styles.time} title={new Date(log.created_at).toLocaleString("zh-CN")}>
                        {formatTime(log.created_at)}
                      </td>
                      <td>
                        <span className={`${styles.actionBadge} ${styles[getActionColor(log.action)]}`}>
                          {getActionLabel(log.action)}
                        </span>
                      </td>
                      <td>{log.user_name || log.user_id || "-"}</td>
                      <td>{log.target_user_name || log.target_user_id || "-"}</td>
                      <td className={styles.ip}>{log.ip_address || "-"}</td>
                      <td>
                        {log.details && (
                          <button
                            className={styles.expandBtn}
                            onClick={() => toggleExpand(log.id)}
                          >
                            {expandedIds.has(log.id) ? "▼" : "▶"}
                          </button>
                        )}
                      </td>
                    </tr>
                    {expandedIds.has(log.id) && log.details && (
                      <tr>
                        <td colSpan={6} className={styles.detailsCell}>
                          <pre className={styles.detailsContent}>
                            {JSON.stringify(log.details, null, 2)}
                          </pre>
                        </td>
                      </tr>
                    )}
                  </>
                ))}
              </tbody>
            </table>
          </div>

          {/* Pagination */}
          {totalPages > 1 && (
            <div className={styles.pagination}>
              <button
                className={styles.pageBtn}
                disabled={page === 1}
                onClick={() => setPage(page - 1)}
              >
                上一页
              </button>
              <span className={styles.pageInfo}>
                第 {page} / {totalPages} 页 · 共 {total} 条日志
              </span>
              <button
                className={styles.pageBtn}
                disabled={page === totalPages}
                onClick={() => setPage(page + 1)}
              >
                下一页
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
