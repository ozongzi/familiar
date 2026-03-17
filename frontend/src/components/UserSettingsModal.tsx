import { useState, useEffect } from "react";
import { api } from "../api/client";
import type { UserSettings, ModelConfig } from "../api/types";
import styles from "./UserSettingsModal.module.css";

interface Props {
  token: string;
  onClose: () => void;
}

export function UserSettingsModal({ token, onClose }: Props) {
  const [settings, setSettings] = useState<UserSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .getSettings(token)
      .then(setSettings)
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, [token]);

  const handleSave = async () => {
    if (!settings) return;
    setSaving(true);
    setError(null);
    try {
      await api.updateSettings(token, {
        frontier_model: settings.frontier_model,
        cheap_model: settings.cheap_model,
        system_prompt: settings.system_prompt,
      });
      onClose();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const updateModel = (
    key: "frontier_model" | "cheap_model",
    field: keyof ModelConfig,
    value: string,
  ) => {
    if (!settings) return;
    setSettings({
      ...settings,
      [key]: {
        ...settings[key],
        [field]: value,
      },
    });
  };

  // loading placeholder will be handled after all hooks are declared,
  // to preserve consistent hook ordering.

  type UserSkill = {
    id: string;
    name: string;
    description?: string | null;
    content: string;
    created_at: string;
  };

  const [skills, setSkills] = useState<UserSkill[]>([]);
  const [skillsLoading, setSkillsLoading] = useState(false);
  const [newSkill, setNewSkill] = useState<{
    name: string;
    description: string;
    content: string;
  }>({
    name: "",
    description: "",
    content: "",
  });
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingSkill, setEditingSkill] = useState<{
    name: string;
    description: string;
    content: string;
  }>({
    name: "",
    description: "",
    content: "",
  });

  useEffect(() => {
    // Load user's skills from the backend when modal opens / token changes.
    // Uses relative path so it works with the same origin. Requires backend routes to be available.
    async function loadSkills() {
      if (!token) return;
      setSkillsLoading(true);
      try {
        const res = await fetch("/api/skills", {
          method: "GET",
          headers: {
            "Content-Type": "application/json",
            Authorization: `Bearer ${token}`,
          },
        });
        if (res.ok) {
          const data = await res.json();
          setSkills(data);
        } else {
          console.warn("failed to load skills", res.status);
        }
      } catch (e) {
        console.warn("loadSkills error", e);
      } finally {
        setSkillsLoading(false);
      }
    }
    loadSkills();
  }, [token, settings?.system_prompt]);

  async function createSkill() {
    if (!token) return;
    try {
      const res = await fetch("/api/skills", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify(newSkill),
      });
      if (res.ok) {
        const created = await res.json();
        setSkills((s) =>
          [...s, created].sort((a, b) => a.name.localeCompare(b.name)),
        );
        setNewSkill({ name: "", description: "", content: "" });
      } else {
        const err = await res.json().catch(() => null);
        console.warn("createSkill failed", res.status, err);
      }
    } catch (e) {
      console.warn("createSkill error", e);
    }
  }

  async function saveEditedSkill() {
    if (!token || !editingId) return;
    try {
      const res = await fetch(`/api/skills/${editingId}`, {
        method: "PUT",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify(editingSkill),
      });
      if (res.ok) {
        const updated = await res.json();
        setSkills((s) =>
          s
            .map((it) => (it.id === updated.id ? updated : it))
            .sort((a, b) => a.name.localeCompare(b.name)),
        );
        setEditingId(null);
        setEditingSkill({ name: "", description: "", content: "" });
      } else {
        const err = await res.json().catch(() => null);
        console.warn("updateSkill failed", res.status, err);
      }
    } catch (e) {
      console.warn("saveEditedSkill error", e);
    }
  }

  async function deleteSkill(id: string) {
    if (!token) return;
    try {
      const res = await fetch(`/api/skills/${id}`, {
        method: "DELETE",
        headers: {
          Authorization: `Bearer ${token}`,
        },
      });
      if (res.ok) {
        setSkills((s) => s.filter((it) => it.id !== id));
      } else {
        console.warn("deleteSkill failed", res.status);
      }
    } catch (e) {
      console.warn("deleteSkill error", e);
    }
  }

  if (loading) return null; // Or a loader

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div className={styles.modal} onClick={(e) => e.stopPropagation()}>
        <div className={styles.header}>
          <h2>
            <SettingsIcon />
            用户设置
          </h2>
          <button className={styles.closeBtn} onClick={onClose}>
            <CloseIcon />
          </button>
        </div>
        <div className={styles.content}>
          {error && (
            <div
              style={{
                color: "var(--danger)",
                marginBottom: "16px",
                fontSize: "0.9rem",
              }}
            >
              {error}
            </div>
          )}
          <div className={styles.section}>
            <h3>主力模型 (Frontier)</h3>
            <div className={styles.field}>
              <label>模型名称</label>
              <input
                value={settings?.frontier_model.name || ""}
                onChange={(e) =>
                  updateModel("frontier_model", "name", e.target.value)
                }
                placeholder="例如: deepseek-chat"
              />
            </div>
            <div className={styles.field}>
              <label>API Base</label>
              <input
                value={settings?.frontier_model.api_base || ""}
                onChange={(e) =>
                  updateModel("frontier_model", "api_base", e.target.value)
                }
                placeholder="https://api.deepseek.com/v1"
              />
            </div>
            <div className={styles.field}>
              <label>API Key</label>
              <input
                type="password"
                value={settings?.frontier_model.api_key || ""}
                onChange={(e) =>
                  updateModel("frontier_model", "api_key", e.target.value)
                }
                placeholder="sk-..."
              />
            </div>
          </div>
          <div className={styles.section}>
            <h3>轻量模型 (Cheap)</h3>
            <div className={styles.field}>
              <label>模型名称</label>
              <input
                value={settings?.cheap_model.name || ""}
                onChange={(e) =>
                  updateModel("cheap_model", "name", e.target.value)
                }
                placeholder="例如: deepseek-chat"
              />
            </div>
            <div className={styles.field}>
              <label>API Base</label>
              <input
                value={settings?.cheap_model.api_base || ""}
                onChange={(e) =>
                  updateModel("cheap_model", "api_base", e.target.value)
                }
                placeholder="https://api.deepseek.com/v1"
              />
            </div>
            <div className={styles.field}>
              <label>API Key</label>
              <input
                type="password"
                value={settings?.cheap_model.api_key || ""}
                onChange={(e) =>
                  updateModel("cheap_model", "api_key", e.target.value)
                }
                placeholder="sk-..."
              />
            </div>
          </div>
          <div className={styles.section}>
            <h3>系统提示词 (System Prompt)</h3>
            <div className={styles.field}>
              <textarea
                value={settings?.system_prompt || ""}
                onChange={(e) =>
                  setSettings((s) =>
                    s ? { ...s, system_prompt: e.target.value } : null,
                  )
                }
                placeholder="输入全局系统提示词..."
              />
            </div>
          </div>

          <div className={styles.section}>
            <h3>Skills 管理</h3>

            {/* 新增 Skill 表单 - 结构向 Model Config 看齐 */}
            <div className={styles.skillForm}>
              <div className={styles.field}>
                <label>名称</label>
                <input
                  placeholder="例如: Python 专家"
                  value={newSkill.name}
                  onChange={(e) =>
                    setNewSkill((s) => ({ ...s, name: e.target.value }))
                  }
                />
              </div>
              <div className={styles.field}>
                <label>描述 (可选)</label>
                <input
                  placeholder="简短描述该 Skill 的用途"
                  value={newSkill.description}
                  onChange={(e) =>
                    setNewSkill((s) => ({ ...s, description: e.target.value }))
                  }
                />
              </div>
              <div className={styles.field}>
                <label>完整内容 (Markdown)</label>
                <textarea
                  placeholder="在此输入 Prompt 详情..."
                  rows={4}
                  value={newSkill.content}
                  onChange={(e) =>
                    setNewSkill((s) => ({ ...s, content: e.target.value }))
                  }
                />
              </div>
              <div className={styles.skillActions}>
                <button
                  className={styles.secondaryBtn}
                  onClick={createSkill}
                  disabled={!newSkill.name || !newSkill.content}
                >
                  添加 Skill
                </button>
                <button
                  className={styles.ghostBtn}
                  onClick={() =>
                    setNewSkill({ name: "", description: "", content: "" })
                  }
                >
                  清空
                </button>
              </div>
            </div>

            <hr className={styles.divider} />

            {/* 已有 Skills 列表 */}
            <div className={styles.skillList}>
              <label className={styles.listLabel}>
                已保存的 Skills ({skills.length})
              </label>
              {skillsLoading ? (
                <div className={styles.infoText}>加载中...</div>
              ) : skills.length === 0 ? (
                <div className={styles.infoText}>无自定义 Skill</div>
              ) : (
                skills.map((s) => (
                  <div key={s.id} className={styles.skillItem}>
                    {editingId === s.id ? (
                      <div className={styles.editMode}>
                        <input
                          className={styles.miniInput}
                          value={editingSkill.name}
                          onChange={(e) =>
                            setEditingSkill((x) => ({
                              ...x,
                              name: e.target.value,
                            }))
                          }
                        />
                        <textarea
                          className={styles.miniTextarea}
                          value={editingSkill.content}
                          onChange={(e) =>
                            setEditingSkill((x) => ({
                              ...x,
                              content: e.target.value,
                            }))
                          }
                        />
                        <div className={styles.skillActions}>
                          <button
                            className={styles.saveBtnSmall}
                            onClick={saveEditedSkill}
                          >
                            保存
                          </button>
                          <button
                            className={styles.cancelBtnSmall}
                            onClick={() => setEditingId(null)}
                          >
                            取消
                          </button>
                        </div>
                      </div>
                    ) : (
                      <>
                        <div className={styles.skillInfo}>
                          <div className={styles.skillName}>{s.name}</div>
                          <div className={styles.skillDesc}>
                            {s.description}
                          </div>
                          <div className={styles.skillPreview}>
                            {s.content.slice(0, 100)}
                            {s.content.length > 100 ? "..." : ""}
                          </div>
                        </div>
                        <div className={styles.skillItemButtons}>
                          <button
                            onClick={() => {
                              setEditingId(s.id);
                              setEditingSkill({
                                name: s.name,
                                description: s.description || "",
                                content: s.content,
                              });
                            }}
                          >
                            编辑
                          </button>
                          <button
                            className={styles.deleteBtn}
                            onClick={() => deleteSkill(s.id)}
                          >
                            删除
                          </button>
                        </div>
                      </>
                    )}
                  </div>
                ))
              )}
            </div>
          </div>
        </div>

        <div className={styles.footer}>
          <button className={styles.cancelBtn} onClick={onClose}>
            取消
          </button>
          <button
            className={styles.saveBtn}
            onClick={handleSave}
            disabled={saving}
          >
            {saving ? "保存中..." : "保存设置"}
          </button>
        </div>
      </div>
    </div>
  );
}

function CloseIcon() {
  return (
    <svg
      width="20"
      height="20"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  );
}

function SettingsIcon() {
  return (
    <svg
      width="20"
      height="20"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}
