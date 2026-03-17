import { useState, useEffect } from "react";
import { api } from "../api/client";
import { UserSettings, ModelConfig } from "../api/types";
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
    api.getSettings(token)
      .then(setSettings)
      .catch(e => setError(e.message))
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

  const updateModel = (key: 'frontier_model' | 'cheap_model', field: keyof ModelConfig, value: string) => {
    if (!settings) return;
    setSettings({
      ...settings,
      [key]: {
        ...settings[key],
        [field]: value
      }
    });
  };

  if (loading) return null; // Or a loader

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div className={styles.modal} onClick={e => e.stopPropagation()}>
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
          {error && <div style={{ color: 'var(--danger)', marginBottom: '16px', fontSize: '0.9rem' }}>{error}</div>}

          <div className={styles.section}>
            <h3>主力模型 (Frontier)</h3>
            <div className={styles.field}>
              <label>模型名称</label>
              <input
                value={settings?.frontier_model.name || ''}
                onChange={e => updateModel('frontier_model', 'name', e.target.value)}
                placeholder="例如: deepseek-chat"
              />
            </div>
            <div className={styles.field}>
              <label>API Base</label>
              <input
                value={settings?.frontier_model.api_base || ''}
                onChange={e => updateModel('frontier_model', 'api_base', e.target.value)}
                placeholder="https://api.deepseek.com/v1"
              />
            </div>
            <div className={styles.field}>
              <label>API Key</label>
              <input
                type="password"
                value={settings?.frontier_model.api_key || ''}
                onChange={e => updateModel('frontier_model', 'api_key', e.target.value)}
                placeholder="sk-..."
              />
            </div>
          </div>

          <div className={styles.section}>
            <h3>轻量模型 (Cheap)</h3>
            <div className={styles.field}>
              <label>模型名称</label>
              <input
                value={settings?.cheap_model.name || ''}
                onChange={e => updateModel('cheap_model', 'name', e.target.value)}
                placeholder="例如: deepseek-chat"
              />
            </div>
            <div className={styles.field}>
              <label>API Base</label>
              <input
                value={settings?.cheap_model.api_base || ''}
                onChange={e => updateModel('cheap_model', 'api_base', e.target.value)}
                placeholder="https://api.deepseek.com/v1"
              />
            </div>
            <div className={styles.field}>
              <label>API Key</label>
              <input
                type="password"
                value={settings?.cheap_model.api_key || ''}
                onChange={e => updateModel('cheap_model', 'api_key', e.target.value)}
                placeholder="sk-..."
              />
            </div>
          </div>

          <div className={styles.section}>
            <h3>系统提示词 (System Prompt)</h3>
            <div className={styles.field}>
              <textarea
                value={settings?.system_prompt || ''}
                onChange={e => setSettings(s => s ? { ...s, system_prompt: e.target.value } : null)}
                placeholder="输入全局系统提示词..."
              />
            </div>
          </div>
        </div>

        <div className={styles.footer}>
          <button className={styles.cancelBtn} onClick={onClose}>取消</button>
          <button
            className={styles.saveBtn}
            onClick={handleSave}
            disabled={saving}
          >
            {saving ? '保存中...' : '保存设置'}
          </button>
        </div>
      </div>
    </div>
  );
}

function CloseIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  );
}

function SettingsIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}
