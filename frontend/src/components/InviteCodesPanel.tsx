import { useState, useEffect, useCallback } from "react";
import styles from "./InviteCodesPanel.module.css";
import { useAuth } from "../store/auth.shared";
import { listInviteCodes, createInviteCode, deleteInviteCode } from "../api/admin";
import type { InviteCode } from "../api/admin";

export function InviteCodesPanel() {
  const { token } = useAuth();
  const [codes, setCodes] = useState<InviteCode[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [expiryDays, setExpiryDays] = useState<string>("");
  const [copied, setCopied] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!token) return;
    setLoading(true);
    setError(null);
    try {
      setCodes(await listInviteCodes(token));
    } catch (e) {
      setError(e instanceof Error ? e.message : "加载失败");
    } finally {
      setLoading(false);
    }
  }, [token]);

  useEffect(() => { load(); }, [load]);

  const handleCreate = async () => {
    if (!token) return;
    setCreating(true);
    setError(null);
    try {
      const days = expiryDays ? parseInt(expiryDays, 10) : null;
      const code = await createInviteCode(days, token);
      setCodes(prev => [code, ...prev]);
      setExpiryDays("");
    } catch (e) {
      setError(e instanceof Error ? e.message : "创建失败");
    } finally {
      setCreating(false);
    }
  };

  const handleDelete = async (code: string) => {
    if (!token) return;
    if (!confirm(`确定要撤销邀请码 ${code} 吗？`)) return;
    try {
      await deleteInviteCode(code, token);
      setCodes(prev => prev.filter(c => c.code !== code));
    } catch (e) {
      alert(e instanceof Error ? e.message : "撤销失败");
    }
  };

  const handleCopy = (code: string) => {
    const url = `${window.location.origin}/#invite=${code}`;
    navigator.clipboard.writeText(url).then(() => {
      setCopied(code);
      setTimeout(() => setCopied(null), 2000);
    });
  };

  const fmt = (d: string | null) =>
    d ? new Date(d).toLocaleDateString("zh-CN") : "—";

  return (
    <div className={styles.panel}>
      <div className={styles.toolbar}>
        <div className={styles.createRow}>
          <input
            className={styles.input}
            type="number"
            min="1"
            placeholder="有效天数（留空=永久）"
            value={expiryDays}
            onChange={e => setExpiryDays(e.target.value)}
          />
          <button className={styles.createBtn} onClick={handleCreate} disabled={creating}>
            {creating ? "生成中…" : "生成邀请码"}
          </button>
        </div>
        {error && <div className={styles.error}>{error}</div>}
      </div>

      {loading ? (
        <div className={styles.empty}>加载中…</div>
      ) : codes.length === 0 ? (
        <div className={styles.empty}>暂无邀请码</div>
      ) : (
        <table className={styles.table}>
          <thead>
            <tr>
              <th>邀请码</th>
              <th>创建时间</th>
              <th>过期时间</th>
              <th>状态</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {codes.map(c => (
              <tr key={c.code} className={c.used_by ? styles.used : ""}>
                <td className={styles.codeCell}>
                  <code>{c.code}</code>
                  {!c.used_by && (
                    <button
                      className={styles.copyBtn}
                      onClick={() => handleCopy(c.code)}
                      title="复制注册链接"
                    >
                      {copied === c.code ? "已复制" : "复制链接"}
                    </button>
                  )}
                </td>
                <td>{fmt(c.created_at)}</td>
                <td>{fmt(c.expires_at)}</td>
                <td>
                  {c.used_by ? (
                    <span className={styles.badgeUsed}>已使用</span>
                  ) : (
                    <span className={styles.badgeActive}>未使用</span>
                  )}
                </td>
                <td>
                  {!c.used_by && (
                    <button
                      className={styles.revokeBtn}
                      onClick={() => handleDelete(c.code)}
                    >
                      撤销
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
