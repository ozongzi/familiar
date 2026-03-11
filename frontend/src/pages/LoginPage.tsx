import { useState, type FormEvent } from "react";
import { api } from "../api/client";
import { useAuth } from "../store/auth.shared";
import styles from "./LoginPage.module.css";

type Mode = "login" | "register";

export function LoginPage() {
  const { login } = useAuth();
  const [mode, setMode] = useState<Mode>("login");
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!name.trim() || !password.trim()) {
      setError("请填写用户名和密码");
      return;
    }
    setError(null);
    setLoading(true);
    try {
      if (mode === "login") {
        const res = await api.login({ name: name.trim(), password });
        await login(res.token);
      } else {
        await api.register({ name: name.trim(), password });
        // Auto-login after register
        const res = await api.login({ name: name.trim(), password });
        await login(res.token);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "请求失败");
    } finally {
      setLoading(false);
    }
  }

  function toggleMode() {
    setMode((m) => (m === "login" ? "register" : "login"));
    setError(null);
  }

  return (
    <div className={styles.page}>
      <div className={styles.card}>
        {/* ── Logo ─────────────────────────────────────── */}
        <div className={styles.logoRow}>
          <img src="/favicon.svg" width={40} height={40} alt="" />
          <h1 className={styles.title}>Familiar</h1>
        </div>
        <p className={styles.subtitle}>
          {mode === "login" ? "欢迎回来" : "创建账号"}
        </p>

        {/* ── Form ─────────────────────────────────────── */}
        <form className={styles.form} onSubmit={handleSubmit} noValidate>
          <div className={styles.field}>
            <label htmlFor="name" className={styles.label}>
              用户名
            </label>
            <input
              id="name"
              className={styles.input}
              type="text"
              autoComplete={mode === "login" ? "username" : "username"}
              autoFocus
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="输入用户名"
              maxLength={40}
              disabled={loading}
            />
          </div>

          <div className={styles.field}>
            <label htmlFor="password" className={styles.label}>
              密码
            </label>
            <input
              id="password"
              className={styles.input}
              type="password"
              autoComplete={
                mode === "login" ? "current-password" : "new-password"
              }
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="输入密码"
              maxLength={100}
              disabled={loading}
            />
          </div>

          {error && (
            <p className={styles.error} role="alert">
              {error}
            </p>
          )}

          <button className={styles.submitBtn} type="submit" disabled={loading}>
            {loading ? "请稍候…" : mode === "login" ? "登录" : "注册"}
          </button>
        </form>

        {/* ── Toggle ───────────────────────────────────── */}
        <p className={styles.toggle}>
          {mode === "login" ? "还没有账号？" : "已有账号？"}
          <button
            className={styles.toggleBtn}
            type="button"
            onClick={toggleMode}
            disabled={loading}
          >
            {mode === "login" ? "注册" : "登录"}
          </button>
        </p>
      </div>
    </div>
  );
}
