import { useState } from "react";
import styles from "./UserFormModal.module.css";
import { useAuth } from "../store/auth.shared";
import { createUser, updateUser } from "../api/admin";
import type { User } from "../api/types";

interface UserFormModalProps {
  mode: "create" | "edit";
  user?: User;
  onClose: () => void;
  onSuccess: () => void;
}

export function UserFormModal({ mode, user, onClose, onSuccess }: UserFormModalProps) {
  const { token } = useAuth();
  const [name, setName] = useState(user?.name || "");
  const [displayName, setDisplayName] = useState(user?.display_name || "");
  const [email, setEmail] = useState(user?.email || "");
  const [password, setPassword] = useState("");
  const [isAdmin, setIsAdmin] = useState(user?.is_admin || false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!token) return;

    setLoading(true);
    setError(null);

    try {
      if (mode === "create") {
        if (!name.trim() || !password.trim()) {
          throw new Error("用户名和密码不能为空");
        }
        if (password.length < 6) {
          throw new Error("密码至少需要6个字符");
        }
        await createUser(
          {
            name: name.trim(),
            email: email.trim() || undefined,
            display_name: displayName.trim() || undefined,
            password,
            is_admin: isAdmin,
          },
          token
        );
      } else if (user) {
        await updateUser(
          user.id,
          {
            email: email.trim() || null,
            display_name: displayName.trim() || null,
            is_admin: isAdmin,
          },
          token
        );
      }
      onSuccess();
    } catch (err) {
      setError(err instanceof Error ? err.message : "操作失败");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div className={styles.modal} onClick={(e) => e.stopPropagation()}>
        <div className={styles.header}>
          <h2>{mode === "create" ? "创建用户" : "编辑用户"}</h2>
          <button className={styles.closeBtn} onClick={onClose}>
            ✕
          </button>
        </div>

        <form onSubmit={handleSubmit} className={styles.content}>
          {error && <div className={styles.error}>{error}</div>}

          <div className={styles.field}>
            <label>用户名 *</label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={mode === "edit"}
              required
              minLength={3}
              maxLength={20}
            />
            {mode === "create" && (
              <span className={styles.hint}>3-20字符，字母数字下划线</span>
            )}
            {mode === "edit" && (
              <span className={styles.hint}>用户名不可修改</span>
            )}
          </div>

          <div className={styles.field}>
            <label>显示名称</label>
            <input
              type="text"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder="留空则使用用户名"
            />
          </div>

          <div className={styles.field}>
            <label>邮箱</label>
            <input
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="可选"
            />
          </div>

          {mode === "create" && (
            <div className={styles.field}>
              <label>密码 *</label>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
                minLength={6}
              />
              <span className={styles.hint}>至少6个字符</span>
            </div>
          )}

          <div className={styles.field}>
            <label className={styles.checkbox}>
              <input
                type="checkbox"
                checked={isAdmin}
                onChange={(e) => setIsAdmin(e.target.checked)}
              />
              <span>管理员权限</span>
            </label>
          </div>

          <div className={styles.footer}>
            <button
              type="button"
              className={styles.cancelBtn}
              onClick={onClose}
              disabled={loading}
            >
              取消
            </button>
            <button
              type="submit"
              className={styles.saveBtn}
              disabled={loading}
            >
              {loading ? "保存中..." : mode === "create" ? "创建" : "保存"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
