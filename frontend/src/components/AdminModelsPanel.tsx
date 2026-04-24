import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { api } from "../api/client";
import type { Model, ReasoningEffort, UpsertModelRequest } from "../api/types";
import { ProviderSelector } from "./ProviderSelector";
import styles from "./AdminModelsPanel.module.css";


const EMPTY_FORM: UpsertModelRequest = {
  label: "",
  provider: "deepseek",
  model_name: "",
  api_base: "",
  api_key: "",
  kind: "api",
  role: null,
  initial_available: true,
  is_default: false,
  compact_trigger_tokens: 50000,
  compact_tail_tokens: 16000,
  reasoning_effort: null,
};

const REASONING_EFFORT_OPTIONS: { value: "" | ReasoningEffort; label: string }[] = [
  { value: "", label: "默认（不设置）" },
  { value: "none", label: "none — 显式关闭思考" },
  { value: "minimal", label: "minimal" },
  { value: "low", label: "low" },
  { value: "medium", label: "medium" },
  { value: "high", label: "high" },
  { value: "xhigh", label: "xhigh" },
  { value: "max", label: "max" },
];

interface Props {
  token: string;
}

export function AdminModelsPanel({ token }: Props) {
  const [models, setModels] = useState<Model[]>([]);
  const [editing, setEditing] = useState<string | null>(null); // id or "new"
  const [form, setForm] = useState<UpsertModelRequest>(EMPTY_FORM);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  async function load() {
    try {
      const data = await api.adminListModels(token);
      setModels(data);
    } catch {
      setError("加载失败");
    }
  }

  useEffect(() => { load(); }, [token]);

  // ESC closes the editor modal.
  useEffect(() => {
    if (!editing) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") cancel();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [editing]);

  function startNew() {
    setForm(EMPTY_FORM);
    setEditing("new");
    setError("");
  }

  function startEdit(m: Model) {
    setForm({
      label: m.label,
      provider: m.provider,
      model_name: m.model_name,
      api_base: m.api_base,
      api_key: "",
      kind: m.kind,
      role: m.role,
      initial_available: m.initial_available,
      is_default: m.is_default,
      compact_trigger_tokens: m.compact_trigger_tokens,
      compact_tail_tokens: m.compact_tail_tokens,
      reasoning_effort: m.reasoning_effort,
    });
    setEditing(m.id);
    setError("");
  }

  function cancel() {
    setEditing(null);
    setError("");
  }

  async function save() {
    if (!form.label.trim() || !form.model_name.trim()) {
      setError("名称和模型名不能为空");
      return;
    }
    setSaving(true);
    setError("");
    try {
      if (editing === "new") {
        const created = await api.adminCreateModel(token, form);
        setModels((prev) => [...prev, created]);
      } else if (editing) {
        const updated = await api.adminUpdateModel(token, editing, form);
        // is_default=true clears other defaults on the backend — mirror that
        // locally so the UI doesn't momentarily show two "默认" badges.
        setModels((prev) =>
          prev.map((m) => {
            if (m.id === updated.id) return updated;
            return updated.is_default ? { ...m, is_default: false } : m;
          })
        );
      }
      setEditing(null);
    } catch (e: any) {
      setError(e.message ?? "保存失败");
    } finally {
      setSaving(false);
    }
  }

  async function remove(id: string) {
    if (!confirm("删除此模型？")) return;
    try {
      await api.adminDeleteModel(token, id);
      setModels((prev) => prev.filter((m) => m.id !== id));
    } catch (e: any) {
      setError(e.message ?? "删除失败");
    }
  }

  const formBody = editing && (
    <div className={styles.form}>
          <div className={styles.section}>
            <div className={styles.sectionTitle}>连接</div>
            <div className={styles.row}>
              <label>显示名称</label>
              <input
                value={form.label}
                onChange={(e) => setForm({ ...form, label: e.target.value })}
                placeholder="e.g. Claude Sonnet"
              />
            </div>
            <div className={styles.row}>
              <label>类型</label>
              <select
                value={form.kind ?? "api"}
                onChange={(e) =>
                  setForm({ ...form, kind: e.target.value as "api" | "claude-code" })
                }
              >
                <option value="api">API (HTTP provider)</option>
                <option value="claude-code">Claude Code (Max OAuth, 本地 claude -p)</option>
              </select>
            </div>
            <div className={styles.row}>
              <label>Provider</label>
              <ProviderSelector value={form.provider} onChange={(p) => setForm({ ...form, provider: p })} />
            </div>
            <div className={styles.row}>
              <label>Model Name</label>
              <input
                value={form.model_name}
                onChange={(e) => setForm({ ...form, model_name: e.target.value })}
                placeholder={form.kind === "claude-code" ? "sonnet / opus / haiku 或完整 model id" : "e.g. claude-sonnet-4-5"}
              />
            </div>
            <div className={styles.row}>
              <label>API Base</label>
              <input
                value={form.api_base}
                onChange={(e) => setForm({ ...form, api_base: e.target.value })}
                placeholder={form.kind === "claude-code" ? "claude-code 不使用 HTTP，此字段可留空" : "留空使用默认"}
                disabled={form.kind === "claude-code"}
              />
            </div>
            <div className={styles.row}>
              <label>API Key</label>
              <input
                type="password"
                value={form.api_key}
                onChange={(e) => setForm({ ...form, api_key: e.target.value })}
                placeholder={
                  form.kind === "claude-code"
                    ? "使用系统 claude CLI 的 Max OAuth，此处留空"
                    : editing !== "new"
                      ? "不修改则留空"
                      : ""
                }
                disabled={form.kind === "claude-code"}
              />
            </div>
          </div>

          <div className={styles.section}>
            <div className={styles.sectionTitle}>角色</div>
            <div className={styles.row}>
              <select
                value={form.role ?? ""}
                onChange={(e) =>
                  setForm({
                    ...form,
                    role: (e.target.value || null) as "cheap" | "embedding" | null,
                  })
                }
              >
                <option value="">无</option>
                <option value="cheap">cheap（spawn / 自动标题等便宜调用）</option>
                <option value="embedding">embedding（向量化）</option>
              </select>
            </div>
          </div>

          <div className={styles.section}>
            <div className={styles.sectionTitle}>Compaction</div>
            <div className={styles.row}>
              <label>Trigger (tokens)</label>
              <input
                type="number"
                min={1000}
                value={form.compact_trigger_tokens}
                onChange={(e) =>
                  setForm({ ...form, compact_trigger_tokens: Number(e.target.value) || 0 })
                }
                placeholder="50000"
              />
            </div>
            <div className={styles.row}>
              <label>Recent tail (tokens)</label>
              <input
                type="number"
                min={1000}
                value={form.compact_tail_tokens}
                onChange={(e) =>
                  setForm({ ...form, compact_tail_tokens: Number(e.target.value) || 0 })
                }
                placeholder="16000"
              />
            </div>
          </div>

          <div className={styles.section}>
            <div className={styles.sectionTitle}>思考强度</div>
            <div className={styles.row}>
              <label>reasoning_effort</label>
              <select
                value={form.reasoning_effort ?? ""}
                onChange={(e) =>
                  setForm({
                    ...form,
                    reasoning_effort:
                      e.target.value === "" ? null : (e.target.value as ReasoningEffort),
                  })
                }
              >
                {REASONING_EFFORT_OPTIONS.map((opt) => (
                  <option key={opt.value || "_default"} value={opt.value}>
                    {opt.label}
                  </option>
                ))}
              </select>
            </div>
          </div>

          <div className={styles.section}>
            <div className={styles.sectionTitle}>访问</div>
            <label className={styles.checkRow}>
              <input
                type="checkbox"
                checked={!!form.is_default}
                onChange={(e) => setForm({ ...form, is_default: e.target.checked })}
              />
              设为默认模型
              <span className={styles.hint}>（同时清除其他默认）</span>
            </label>
            <label className={styles.checkRow}>
              <input
                type="checkbox"
                checked={form.initial_available ?? true}
                onChange={(e) => setForm({ ...form, initial_available: e.target.checked })}
              />
              初始可用
              <span className={styles.hint}>（可在模型权限矩阵里按用户覆盖）</span>
            </label>
          </div>

          <div className={styles.actions}>
            <button className={styles.saveBtn} onClick={save} disabled={saving}>
              {saving ? "保存中…" : "保存"}
            </button>
            <button className={styles.cancelBtn} onClick={cancel}>取消</button>
          </div>
        </div>
  );

  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        <h3>全局模型</h3>
        <button className={styles.addBtn} onClick={startNew}>+ 添加</button>
      </div>

      {error && <p className={styles.error}>{error}</p>}

      <div className={styles.list}>
        {models.length === 0 && (
          <p className={styles.empty}>暂无全局模型，点击"添加"创建</p>
        )}
        {models.map((m) => (
          <div key={m.id} className={`${styles.item} ${m.is_default ? styles.itemDefault : ""}`}>
            <div className={styles.itemHeader}>
              <span className={styles.itemLabel}>{m.label}</span>
              <div className={styles.itemBadges}>
                {m.is_default && <span className={styles.defaultBadge}>默认</span>}
                {m.role === "cheap" && <span className={styles.roleBadge}>cheap</span>}
                {m.role === "embedding" && <span className={styles.roleBadge}>embedding</span>}
                {m.kind === "claude-code" && <span className={styles.roleBadge}>claude-code</span>}
                {!m.initial_available && <span className={styles.roleBadge}>初始不可用</span>}
              </div>
              <span className={styles.itemMeta}>{m.provider} · {m.model_name}</span>
            </div>
            <div className={styles.itemActions}>
              <button onClick={() => startEdit(m)}>编辑</button>
              <button className={styles.deleteBtn} onClick={() => remove(m.id)}>删除</button>
            </div>
          </div>
        ))}
      </div>

      {editing &&
        createPortal(
          <div
            className={styles.modalOverlay}
            onClick={(e) => {
              if (e.target === e.currentTarget) cancel();
            }}
          >
            <div className={styles.modal}>
              <h4 className={styles.modalTitle}>
                {editing === "new" ? "添加模型" : "编辑模型"}
              </h4>
              {formBody}
            </div>
          </div>,
          document.body,
        )}
    </div>
  );
}
