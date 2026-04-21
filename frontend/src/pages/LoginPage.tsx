import { useEffect, useState } from "react";
import styles from "./LoginPage.module.css";

declare global {
  interface Window {
    __TAURI__?: unknown;
  }
}

const isTauri = () => typeof window.__TAURI__ !== "undefined";

interface LoginPageProps {
  serverUrl?: string;
  onLogin?: (token: string, isNew: boolean) => void;
}

type Mode = "github" | "login" | "register";

export function LoginPage({ serverUrl = "", onLogin }: LoginPageProps) {
  const [mode, setMode] = useState<Mode>("github");
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [inviteCode, setInviteCode] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [bootstrap, setBootstrap] = useState(false);

  // Check whether the server has any users. If not, this is the first-run
  // "become admin" window — skip the landing page and drop straight into
  // register without an invite code.
  useEffect(() => {
    const base = serverUrl || "";
    fetch(`${base}/api/auth/status`)
      .then(r => r.ok ? r.json() : null)
      .then((data: { users_exist: boolean } | null) => {
        if (data && !data.users_exist) {
          setBootstrap(true);
          setMode("register");
        }
      })
      .catch(() => { /* endpoint unavailable — keep default flow */ });
  }, [serverUrl]);

  // Pre-fill invite code from URL hash: /#invite=xxxx
  useEffect(() => {
    const hash = window.location.hash.replace(/^#/, "");
    const match = hash.match(/(?:^|&)invite=([^&]+)/);
    if (match) {
      setInviteCode(match[1]);
      setMode("register");
      window.history.replaceState({}, "", window.location.pathname + window.location.search);
    }
  }, []);

  // Listen for deep-link auth event (Tauri)
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    const setup = async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<string>("familiar-auth", (event) => {
        const url = new URL(event.payload);
        const token = url.searchParams.get("token");
        const isNew = url.searchParams.get("is_new") === "1";
        if (token && onLogin) onLogin(token, isNew);
      });
    };
    setup();
    return () => { unlisten?.(); };
  }, [onLogin]);

  const handleGithubLogin = async (e: React.MouseEvent) => {
    if (!isTauri()) return;
    e.preventDefault();
    const base = serverUrl || window.location.origin;
    const url = `${base}/api/auth/github?client=tauri`;
    if (/android/i.test(navigator.userAgent)) {
      window.open(url, "_blank");
      return;
    }
    const { open } = await import("@tauri-apps/plugin-shell");
    await open(url);
  };

  const handlePasswordLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      const base = serverUrl || "";
      const res = await fetch(`${base}/api/sessions`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: name.trim(), password }),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
      onLogin?.(data.token, false);
    } catch (err) {
      setError(err instanceof Error ? err.message : "登录失败");
    } finally {
      setSubmitting(false);
    }
  };

  const handleRegister = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      const base = serverUrl || "";
      const body: { name: string; password: string; invite_code?: string } = {
        name: name.trim(),
        password,
      };
      if (!bootstrap) body.invite_code = inviteCode.trim();
      const res = await fetch(`${base}/api/auth/register`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
      onLogin?.(data.token, true);
    } catch (err) {
      setError(err instanceof Error ? err.message : "注册失败");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className={styles.page}>
      <div className={styles.card}>
        <div className={styles.logoRow}>
          <img src="/favicon.svg" width={40} height={40} alt="" />
          <h1 className={styles.title}>Familiar</h1>
        </div>
        <p className={styles.subtitle}>
          {bootstrap ? "首次启动 · 注册即成为管理员" : "你的 AI 助手"}
        </p>

        {bootstrap && (
          <div className={styles.warning}>
            ⚠️ 任何访问到此页面的人都会成为管理员。
            如果此服务器已对公网开放，请立即关停，设置
            <code>INITIAL_ADMIN_USERNAME</code> 和
            <code>INITIAL_ADMIN_PASSWORD</code> 环境变量后再启动。
          </div>
        )}

        {mode === "github" && !bootstrap ? (
          <>
            <a
              className={styles.githubBtn}
              href="/api/auth/github"
              onClick={isTauri() ? handleGithubLogin : undefined}
            >
              <svg height="20" width="20" viewBox="0 0 16 16" fill="currentColor">
                <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38
                  0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13
                  -.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66
                  .07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15
                  -.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0
                  1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82
                  1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01
                  1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/>
              </svg>
              使用 GitHub 登录
            </a>
            <div className={styles.toggle}>
              <button className={styles.toggleBtn} onClick={() => setMode("login")}>
                账号密码登录
              </button>
              ·
              <button className={styles.toggleBtn} onClick={() => setMode("register")}>
                邀请码注册
              </button>
            </div>
          </>
        ) : mode === "login" ? (
          <>
            <form className={styles.form} onSubmit={handlePasswordLogin}>
              <div className={styles.field}>
                <label className={styles.label}>用户名</label>
                <input
                  className={styles.input}
                  type="text"
                  placeholder="用户名"
                  value={name}
                  onChange={e => setName(e.target.value)}
                  required
                  autoComplete="username"
                  disabled={submitting}
                />
              </div>
              <div className={styles.field}>
                <label className={styles.label}>密码</label>
                <input
                  className={styles.input}
                  type="password"
                  placeholder="密码"
                  value={password}
                  onChange={e => setPassword(e.target.value)}
                  required
                  autoComplete="current-password"
                  disabled={submitting}
                />
              </div>
              {error && <div className={styles.error}>{error}</div>}
              <button className={styles.submitBtn} type="submit" disabled={submitting}>
                {submitting ? "登录中…" : "登录"}
              </button>
            </form>
            <div className={styles.toggle}>
              <button className={styles.toggleBtn} onClick={() => { setMode("github"); setError(null); }}>
                返回 GitHub 登录
              </button>
            </div>
          </>
        ) : (
          <>
            <form className={styles.form} onSubmit={handleRegister}>
              <div className={styles.field}>
                <label className={styles.label}>用户名</label>
                <input
                  className={styles.input}
                  type="text"
                  placeholder="字母、数字、_ 或 -"
                  value={name}
                  onChange={e => setName(e.target.value)}
                  required
                  autoComplete="username"
                  disabled={submitting}
                />
              </div>
              <div className={styles.field}>
                <label className={styles.label}>密码</label>
                <input
                  className={styles.input}
                  type="password"
                  placeholder="至少 8 位"
                  value={password}
                  onChange={e => setPassword(e.target.value)}
                  required
                  autoComplete="new-password"
                  disabled={submitting}
                />
              </div>
              {!bootstrap && (
                <div className={styles.field}>
                  <label className={styles.label}>邀请码</label>
                  <input
                    className={styles.input}
                    type="text"
                    placeholder="管理员提供的邀请码"
                    value={inviteCode}
                    onChange={e => setInviteCode(e.target.value)}
                    required
                    disabled={submitting}
                  />
                </div>
              )}
              {error && <div className={styles.error}>{error}</div>}
              <button className={styles.submitBtn} type="submit" disabled={submitting}>
                {submitting ? (bootstrap ? "创建中…" : "注册中…") : (bootstrap ? "创建管理员" : "注册")}
              </button>
            </form>
            {!bootstrap && (
              <div className={styles.toggle}>
                <button className={styles.toggleBtn} onClick={() => { setMode("github"); setError(null); }}>
                  返回 GitHub 登录
                </button>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
