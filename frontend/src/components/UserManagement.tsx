import { useState, useEffect } from "react";
import styles from "./UserManagement.module.css";
import { useAuth } from "../store/auth.shared";
import { listUsers, deleteUser } from "../api/admin";
import type { User } from "../api/types";
import { UserFormModal } from "./UserFormModal";
import { PasswordResetModal } from "./PasswordResetModal";
import { Avatar } from "./Avatar";

export function UserManagement() {
  const { token } = useAuth();
  const [users, setUsers] = useState<User[]>([]);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(1);
  const [perPage] = useState(20);
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [editingUser, setEditingUser] = useState<User | null>(null);
  const [resetPasswordUser, setResetPasswordUser] = useState<User | null>(null);

  const loadUsers = async () => {
    if (!token) return;
    setLoading(true);
    setError(null);
    try {
      const result = await listUsers({ page, per_page: perPage, search }, token);
      setUsers(result.items);
      setTotal(result.total);
    } catch (err) {
      setError(err instanceof Error ? err.message : "加载用户失败");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadUsers();
  }, [page, search]);

  const handleDelete = async (user: User) => {
    if (!token) return;
    if (!confirm(`确定要删除用户 "${user.name}" 吗？这将删除该用户的所有数据。`)) {
      return;
    }

    try {
      await deleteUser(user.id, token);
      loadUsers();
    } catch (err) {
      alert(err instanceof Error ? err.message : "删除失败");
    }
  };

  const totalPages = Math.ceil(total / perPage);

  const formatDate = (dateStr: string) => {
    const date = new Date(dateStr);
    return date.toLocaleString("zh-CN");
  };

  const formatLastLogin = (dateStr: string | null | undefined) => {
    if (!dateStr) return "从未登录";
    const date = new Date(dateStr);
    const now = new Date();
    const diff = now.getTime() - date.getTime();
    const minutes = Math.floor(diff / 60000);
    if (minutes < 60) return `${minutes}分钟前`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours}小时前`;
    const days = Math.floor(hours / 24);
    if (days < 7) return `${days}天前`;
    return date.toLocaleDateString("zh-CN");
  };

  return (
    <div className={styles.container}>
      {/* Toolbar */}
      <div className={styles.toolbar}>
        <input
          type="text"
          className={styles.searchBox}
          placeholder="搜索用户名或邮箱..."
          value={search}
          onChange={(e) => {
            setSearch(e.target.value);
            setPage(1);
          }}
        />
        <button
          className={styles.createBtn}
          onClick={() => setShowCreateModal(true)}
        >
          + 创建用户
        </button>
      </div>

      {/* Error */}
      {error && <div className={styles.error}>{error}</div>}

      {/* Table */}
      {loading ? (
        <div className={styles.loading}>加载中...</div>
      ) : users.length === 0 ? (
        <div className={styles.empty}>
          {search ? "没有找到匹配的用户" : "还没有用户"}
        </div>
      ) : (
        <>
          <div className={styles.tableWrapper}>
            <table className={styles.table}>
              <thead className={styles.thead}>
                <tr>
                  <th>头像</th>
                  <th>用户名</th>
                  <th>显示名称</th>
                  <th>邮箱</th>
                  <th>角色</th>
                  <th>最后登录</th>
                  <th>创建时间</th>
                  <th>操作</th>
                </tr>
              </thead>
              <tbody className={styles.tbody}>
                {users.map((user) => (
                  <tr key={user.id} className={styles.row}>
                    <td>
                      <Avatar user={user} size="sm" />
                    </td>
                    <td className={styles.username}>{user.name}</td>
                    <td>{user.display_name || "-"}</td>
                    <td className={styles.email}>{user.email || "-"}</td>
                    <td>
                      <span
                        className={`${styles.badge} ${
                          user.is_admin ? styles.badgeAdmin : styles.badgeUser
                        }`}
                      >
                        {user.is_admin ? "管理员" : "用户"}
                      </span>
                    </td>
                    <td className={styles.lastLogin}>
                      {formatLastLogin(user.last_login_at)}
                    </td>
                    <td className={styles.date}>{formatDate(user.created_at)}</td>
                    <td>
                      <div className={styles.actions}>
                        <button
                          className={styles.actionBtn}
                          onClick={() => setEditingUser(user)}
                          title="编辑"
                        >
                          ✏️
                        </button>
                        <button
                          className={styles.actionBtn}
                          onClick={() => setResetPasswordUser(user)}
                          title="重置密码"
                        >
                          🔑
                        </button>
                        <button
                          className={`${styles.actionBtn} ${styles.deleteBtn}`}
                          onClick={() => handleDelete(user)}
                          title="删除"
                        >
                          🗑️
                        </button>
                      </div>
                    </td>
                  </tr>
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
                第 {page} / {totalPages} 页 · 共 {total} 个用户
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

      {/* Modals */}
      {showCreateModal && (
        <UserFormModal
          mode="create"
          onClose={() => setShowCreateModal(false)}
          onSuccess={() => {
            setShowCreateModal(false);
            loadUsers();
          }}
        />
      )}

      {editingUser && (
        <UserFormModal
          mode="edit"
          user={editingUser}
          onClose={() => setEditingUser(null)}
          onSuccess={() => {
            setEditingUser(null);
            loadUsers();
          }}
        />
      )}

      {resetPasswordUser && (
        <PasswordResetModal
          user={resetPasswordUser}
          onClose={() => setResetPasswordUser(null)}
          onSuccess={() => {
            setResetPasswordUser(null);
          }}
        />
      )}
    </div>
  );
}
