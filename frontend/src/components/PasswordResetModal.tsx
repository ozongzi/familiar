import { useState } from "react";
import styles from "./UserFormModal.module.css";
import { useAuth } from "../store/auth.shared";
import { resetPassword } from "../api/admin";
import type { User } from "../api/types";

interface PasswordResetModalProps {
  user: User;
  onClose: () => void;
  onSuccess: () => void;
}

export function PasswordResetModal({ user, onClose, onSuccess }: PasswordResetModalProps) {
  const { token } = useAuth();
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!token) return;

    if (newPassword !== confirmPassword) {
      setError("两次输入的密码不一致");
      return;
    }

    if (newPassword.length < 6) {
      setError("密码至少需要6个字符");
      return;
    }

    setLoading(true);
    setError(null);

    try {
      await resetPassword(user.id, newPassword, token);
      alert("密码重置成功");
      onSuccess();
    } catch (err) {
      setError(err instanceof Error ? err.message : "重置失败");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div className={styles.modal} onClick={(e) => e.stopPropagation()}>
        <div className={styles.header}>
          <h2>重置密码</h2>
          <button className={styles.closeBtn} onClick={onClose}>
            ✕
          </button>
        </div>

        <form onSubmit={handleSubmit} className={styles.content}>
          {error && <div className={styles.error}>{error}</div>}

          <div className={styles.field}>
            <label>用户</label>
            <input
              type="text"
              value={user.name}
              disabled
            />
          </div>

          <div className={styles.field}>
            <label>新密码 *</label>
            <input
              type="password"
              value={newPassword}
              onChange={(e) => setNewPassword(e.target.value)}
              required
              minLength={6}
              autoFocus
            />
          </div>

          <div className={styles.field}>
            <label>确认密码 *</label>
            <input
              type="password"
              value={confirmPassword}
              onChange={(e) => setConfirmPassword(e.target.value)}
              required
              minLength={6}
            />
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
              {loading ? "重置中..." : "重置密码"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
