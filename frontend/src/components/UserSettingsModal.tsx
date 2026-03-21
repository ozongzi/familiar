import { useMemo, useState, useEffect } from "react";
import hljs from "highlight.js";
import { api } from "../api/client";
import { getProfile, updateProfile, updatePassword, uploadAvatar } from "../api/profile";
import type { UserSettings, Provider } from "../api/types";
import styles from "./UserSettingsModal.module.css";
import "highlight.js/styles/github.css";

const PROVIDER_LABELS: Record<Provider, string> = {
  deepseek: "DeepSeek",
  openai: "OpenAI",
  anthropic: "Anthropic",
  gemini: "Gemini",
};

const PROVIDER_DEFAULTS: Record<Provider, string> = {
  deepseek:  "https://api.deepseek.com/v1",
  openai:    "https://api.openai.com/v1",
  anthropic: "https://api.anthropic.com",
  gemini:    "https://generativelanguage.googleapis.com/v1beta",
};

interface Props {
  token: string;
  onClose: () => void;
}

export function UserSettingsModal({ token, onClose }: Props) {
  const [activeTab, setActiveTab] = useState<"general" | "profile">("profile");
  
  // General Settings State
  const [settings, setSettings] = useState<UserSettings | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  
  // Profile Settings State
  const [profileData, setProfileData] = useState<{
    displayName: string;
    email: string;
  }>({ displayName: "", email: "" });
  const [avatarPreview, setAvatarPreview] = useState<string | null>(null);
  const [avatarFile, setAvatarFile] = useState<File | null>(null);
  
  // Password Change State
  const [passwordForm, setPasswordForm] = useState({
    currentPassword: "",
    newPassword: "",
    confirmPassword: "",
  });

  // Load User Settings
  useEffect(() => {
    if (activeTab === "general") {
      api.getSettings(token).then(setSettings).catch((err) => {
        console.error(err);
        setError("加载设置失败");
      });
    }
  }, [token, activeTab]);

  // Load Profile Data
  useEffect(() => {
    if (activeTab === "profile") {
      getProfile(token)
        .then((u) => {
          setProfileData({
            displayName: u.display_name || "",
            email: u.email || "",
          });
          if (u.avatar_path) {
             setAvatarPreview(`/api/avatars/${u.id}`);
          }
        })
        .catch((e) => setError(e instanceof Error ? e.message : "加载个人资料失败"));
    }
  }, [token, activeTab]);

  // Save General Settings
  async function handleSaveSettings() {
    if (!settings) return;
    setSaving(true);
    setError("");
    try {
      await api.updateSettings(token, {
        mode: settings.mode,
        api_key: settings.api_key,
        api_base: settings.api_base,
        model_name: settings.model_name,
        provider: settings.provider,
        system_prompt: settings.system_prompt,
      });
      onClose();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : "保存失败");
    } finally {
      setSaving(false);
    }
  }

  // Save Profile
  async function handleProfileSave() {
    setSaving(true);
    setError("");
    try {
      await updateProfile({
        display_name: profileData.displayName || null,
        email: profileData.email || null,
      }, token);
      alert("个人资料更新成功");
    } catch (e) {
      setError(e instanceof Error ? e.message : "更新失败");
    } finally {
      setSaving(false);
    }
  }

  async function handlePasswordSave() {
    if (passwordForm.newPassword !== passwordForm.confirmPassword) {
      setError("两次输入的密码不一致");
      return;
    }
    setSaving(true);
    setError("");
    try {
      await updatePassword({
        current_password: passwordForm.currentPassword,
        new_password: passwordForm.newPassword,
      }, token);
      alert("密码修改成功");
      setPasswordForm({ currentPassword: "", newPassword: "", confirmPassword: "" });
    } catch (e) {
      setError(e instanceof Error ? e.message : "密码修改失败");
    } finally {
      setSaving(false);
    }
  }

  function handleAvatarSelect(e: React.ChangeEvent<HTMLInputElement>) {
    if (e.target.files && e.target.files[0]) {
      const file = e.target.files[0];
      setAvatarFile(file);
      
      // Preview
      const reader = new FileReader();
      reader.onload = (evt) => {
        setAvatarPreview(evt.target?.result as string);
      };
      reader.readAsDataURL(file);
    }
  }

  async function handleAvatarUpload() {
    if (!avatarFile) return;
    setSaving(true);
    try {
      await uploadAvatar(avatarFile, token);
      setAvatarFile(null);
      alert("头像上传成功");
    } catch (e) {
      setError(e instanceof Error ? e.message : "头像上传失败");
    } finally {
      setSaving(false);
    }
  }

  // Highlight logic for system prompt
  const highlightedPrompt = useMemo(() => {
    if (!settings?.system_prompt) return "";
    try {
      return hljs.highlight(settings.system_prompt, { language: "markdown" }).value;
    } catch {
      return settings.system_prompt;
    }
  }, [settings?.system_prompt]);

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div className={styles.modal} onClick={(e) => e.stopPropagation()}>
        <div className={styles.header}>
          <h2>设置</h2>
          <button className={styles.closeBtn} onClick={onClose} title="关闭">
            ✕
          </button>
        </div>
        
        <div className={styles.modalBody}>
          <div className={styles.sidebar}>
            <div className={styles.sidebarTitle}>配置选项</div>
            <button
              className={`${styles.tabBtn} ${activeTab === "general" ? styles.active : ""}`}
              onClick={() => setActiveTab("general")}
            >
              通用配置
            </button>
            <button
              className={`${styles.tabBtn} ${activeTab === "profile" ? styles.active : ""}`}
              onClick={() => setActiveTab("profile")}
            >
              个人资料
            </button>
          </div>

          <div className={styles.content}>
          {error && <div className={styles.error}>{error}</div>}

          {activeTab === "general" && settings && (
            <div className={styles.section}>
              <h3>模型配置</h3>
              
              <div className={styles.checkboxRow}>
                <input
                  type="checkbox"
                  id="custom-config"
                  checked={settings.mode === "custom"}
                  onChange={(e) =>
                    setSettings({
                      ...settings,
                      mode: e.target.checked ? "custom" : "default",
                    })
                  }
                />
                <label htmlFor="custom-config">启用自定义配置 (Enable Custom Config)</label>
              </div>

              {settings.mode === "custom" && (
                <div className={styles.customConfig}>
                  <div className={styles.field}>
                    <label>Provider</label>
                    <div className={styles.providerToggle}>
                      {(Object.keys(PROVIDER_LABELS) as Provider[]).map((p) => (
                        <button
                          key={p}
                          className={`${styles.providerBtn} ${
                            (settings.provider ?? "deepseek") === p ? styles.providerBtnActive : ""
                          }`}
                          onClick={() =>
                            setSettings({
                              ...settings,
                              provider: p,
                              api_base: PROVIDER_DEFAULTS[p],
                            })
                          }
                        >
                          {PROVIDER_LABELS[p]}
                        </button>
                      ))}
                    </div>
                  </div>

                  <div className={styles.field}>
                    <label>API Base</label>
                    <input
                      type="text"
                      value={settings.api_base || ""}
                      onChange={(e) =>
                        setSettings({ ...settings, api_base: e.target.value })
                      }
                      placeholder="https://api.deepseek.com/v1"
                    />
                  </div>
                  
                  <div className={styles.field}>
                    <label>API Key</label>
                    <input
                      type="password"
                      value={settings.api_key || ""}
                      onChange={(e) =>
                        setSettings({ ...settings, api_key: e.target.value })
                      }
                      placeholder="sk-..."
                    />
                  </div>

                  <div className={styles.field}>
                    <label>Model Name</label>
                    <input
                      type="text"
                      value={settings.model_name || ""}
                      onChange={(e) =>
                        setSettings({ ...settings, model_name: e.target.value })
                      }
                      placeholder="deepseek-chat"
                    />
                  </div>

                  <div className={styles.field}>
                    <label>System Prompt</label>
                    <div className={styles.editorContainer}>
                      <textarea
                        className={styles.editorTextarea}
                        value={settings.system_prompt || ""}
                        onChange={(e) =>
                          setSettings({ ...settings, system_prompt: e.target.value })
                        }
                        onScroll={(e) => {
                          const target = e.currentTarget;
                          const highlight = target.nextElementSibling as HTMLElement;
                          if (highlight) {
                            highlight.scrollTop = target.scrollTop;
                            highlight.scrollLeft = target.scrollLeft;
                          }
                        }}
                        placeholder="输入系统提示词..."
                        spellCheck={false}
                      />
                      <pre
                        className={styles.editorHighlight}
                        aria-hidden="true"
                        dangerouslySetInnerHTML={{ __html: highlightedPrompt + "\n" }} 
                      />
                    </div>
                  </div>
                </div>
              )}

              <div className={styles.actions}>
                <button
                  className={styles.saveBtn}
                  onClick={handleSaveSettings}
                  disabled={saving}
                >
                  {saving ? "保存中..." : "保存配置"}
                </button>
              </div>
            </div>
          )}

          {activeTab === "profile" && (
            <div className={styles.section}>
              <h3>头像</h3>
              <div className={styles.avatarSection}>
                {avatarPreview ? (
                  <img src={avatarPreview} alt="Avatar preview" className={styles.avatarPreview} />
                ) : (
                  <div className={styles.avatarPlaceholder}>
                    {profileData.displayName?.charAt(0) || "?"}
                  </div>
                )}
                <div className={styles.avatarActions}>
                  <input
                    type="file"
                    id="avatar-upload"
                    accept="image/jpeg,image/png,image/webp"
                    style={{ display: "none" }}
                    onChange={handleAvatarSelect}
                  />
                  <label htmlFor="avatar-upload" className={styles.uploadLabel}>
                    选择头像
                  </label>
                  {avatarFile && (
                    <button onClick={handleAvatarUpload} disabled={saving} className={styles.uploadBtn}>
                      {saving ? "上传中..." : "上传"}
                    </button>
                  )}
                  <p className={styles.hint}>支持 JPG、PNG、WebP，最大 2MB</p>
                </div>
              </div>

              <hr className={styles.divider} />

              <h3>基本信息</h3>
              <div className={styles.field}>
                <label>显示名称</label>
                <input
                  type="text"
                  value={profileData.displayName}
                  onChange={(e) => setProfileData({ ...profileData, displayName: e.target.value })}
                />
              </div>

              <div className={styles.field}>
                <label>邮箱</label>
                <input
                  type="email"
                  value={profileData.email}
                  onChange={(e) => setProfileData({ ...profileData, email: e.target.value })}
                />
              </div>

              <div className={styles.actions}>
                <button onClick={handleProfileSave} disabled={saving} className={styles.saveBtn}>
                  {saving ? "保存中..." : "更新资料"}
                </button>
              </div>

              <hr className={styles.divider} />

              <h3>修改密码</h3>
              <div className={styles.field}>
                <label>当前密码</label>
                <input
                  type="password"
                  value={passwordForm.currentPassword}
                  onChange={(e) => setPasswordForm({ ...passwordForm, currentPassword: e.target.value })}
                />
              </div>
              <div className={styles.field}>
                <label>新密码</label>
                <input
                  type="password"
                  value={passwordForm.newPassword}
                  onChange={(e) => setPasswordForm({ ...passwordForm, newPassword: e.target.value })}
                />
              </div>
              <div className={styles.field}>
                <label>确认新密码</label>
                <input
                  type="password"
                  value={passwordForm.confirmPassword}
                  onChange={(e) => setPasswordForm({ ...passwordForm, confirmPassword: e.target.value })}
                />
              </div>

              <div className={styles.actions}>
                <button onClick={handlePasswordSave} disabled={saving} className={styles.saveBtn}>
                  {saving ? "保存中..." : "修改密码"}
                </button>
              </div>
            </div>
          )}
          </div>
        </div>
      </div>
    </div>
  );
}
