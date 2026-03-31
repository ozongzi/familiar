import { useEffect, useRef, useState } from "react";
import { api } from "../api/client";
import type { Model } from "../api/types";
import styles from "./ModelPicker.module.css";

interface Props {
  token: string;
  value: string | null;
  onChange: (modelId: string | null) => void;
}

export function ModelPicker({ token, value, onChange }: Props) {
  const [models, setModels] = useState<Model[]>([]);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api.listModels(token).then(setModels).catch(() => {});
  }, [token]);

  useEffect(() => {
    if (!open) return;
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [open]);

  if (models.length === 0) return null;

  const selected = models.find((m) => m.id === value);
  const label = selected ? selected.label : "默认模型";

  return (
    <div className={styles.root} ref={ref}>
      <button
        className={styles.trigger}
        onClick={() => setOpen((o) => !o)}
        type="button"
      >
        <span className={styles.triggerIcon}>⚡</span>
        <span className={styles.triggerLabel}>{label}</span>
        <span className={styles.triggerChevron}>{open ? "▲" : "▼"}</span>
      </button>

      {open && (
        <div className={styles.dropdown}>
          <button
            className={`${styles.option} ${value === null ? styles.optionActive : ""}`}
            onClick={() => { onChange(null); setOpen(false); }}
            type="button"
          >
            默认模型
          </button>
          {models.map((m) => (
            <button
              key={m.id}
              className={`${styles.option} ${value === m.id ? styles.optionActive : ""}`}
              onClick={() => { onChange(m.id); setOpen(false); }}
              type="button"
            >
              <span className={styles.optionLabel}>{m.label}</span>
              {m.scope === "user" && <span className={styles.optionTag}>我的</span>}
              {m.is_default && <span className={styles.optionTag}>默认</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
