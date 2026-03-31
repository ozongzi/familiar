import type { Provider } from "../api/types";
import { PROVIDERS, PROVIDER_LABELS } from "../constants/providers";

interface Props {
  value: Provider;
  onChange: (p: Provider) => void;
  variant?: "buttons" | "select";
}

export function ProviderSelector({ value, onChange, variant = "select" }: Props) {
  if (variant === "buttons") {
    return (
      <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
        {PROVIDERS.map((p) => (
          <button
            key={p}
            type="button"
            onClick={() => onChange(p)}
            style={{
              padding: "4px 12px",
              borderRadius: 6,
              border: "1px solid var(--border-subtle)",
              background: value === p ? "var(--accent)" : "var(--bg-surface)",
              color: value === p ? "#fff" : "var(--text-primary)",
              fontWeight: value === p ? 600 : 400,
              cursor: "pointer",
              fontSize: 13,
              transition: "background 120ms, color 120ms",
            }}
          >
            {PROVIDER_LABELS[p]}
          </button>
        ))}
      </div>
    );
  }

  return (
    <select value={value} onChange={(e) => onChange(e.target.value as Provider)}>
      {PROVIDERS.map((p) => (
        <option key={p} value={p}>{PROVIDER_LABELS[p]}</option>
      ))}
    </select>
  );
}
