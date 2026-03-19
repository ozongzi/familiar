import React from "react";
import styles from "./ToolShared.module.css";

export function ToolHeader({
  pending,
  label,
  badges,
  expanded,
  onToggle,
  icon,
}: {
  pending: boolean;
  label: string;
  badges?: React.ReactNode;
  expanded: boolean;
  onToggle: () => void;
  icon: React.ReactNode;
}) {
  return (
    <button className={styles.header} onClick={onToggle} aria-expanded={expanded}>
      <span className={styles.headerIcon}>{icon}</span>
      <span className={`${styles.headerLabel} ${pending ? styles.headerLabelPending : ""}`}>
        {label}
      </span>
      <span className={styles.headerBadges}>{badges}</span>
      {!pending && (
        <span className={`${styles.chevron} ${expanded ? styles.chevronOpen : ""}`}>
          <ChevronIcon />
        </span>
      )}
    </button>
  );
}

export function ExitBadge({ code }: { code: number }) {
  const ok = code === 0;
  return (
    <span className={ok ? styles.badgeOk : styles.badgeErr}>
      exit {code}
    </span>
  );
}

export function CheckBadge({
  success,
  warnings,
  errors,
}: {
  success: boolean;
  warnings: number;
  errors: number;
}) {
  if (errors > 0)
    return <span className={styles.badgeErr}>{errors} error{errors > 1 ? "s" : ""}</span>;
  if (warnings > 0)
    return <span className={styles.badgeWarn}>{warnings} warning{warnings > 1 ? "s" : ""}</span>;
  if (success)
    return <span className={styles.badgeOk}>✓ passed</span>;
  return null;
}

export function PendingBadge() {
  return <span className={styles.badgePending}>실행 중</span>;
}

export function ToolBodyWrap({ children }: { children: React.ReactNode }) {
  return <div className={styles.body}>{children}</div>;
}

export function ChevronIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
      <polyline points="3,5 6,8 9,5" />
    </svg>
  );
}

export function FileEditIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
      <path d="M11 2H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h6a1 1 0 0 0 1-1V5l-3-3z" />
      <polyline points="11,2 11,5 14,5" />
    </svg>
  );
}

export function TerminalIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
      <polyline points="4,6 7,8 4,10" />
      <line x1="9" y1="10" x2="13" y2="10" />
    </svg>
  );
}

export function parseAutocheckResult(result: unknown): {
  success: boolean;
  warnings: Array<{ file: string; line: number; message: string }>;
  errors: Array<{ file: string; line: number; message: string }>;
} | null {
  let r: Record<string, unknown> | null = null;
  if (result && typeof result === "object") {
    const raw = result as Record<string, unknown>;
    if (typeof raw.text === "string") {
      try { r = JSON.parse(raw.text) as Record<string, unknown>; } catch { r = raw; }
    } else {
      r = raw;
    }
  }
  if (!r) return null;
  // autocheck wraps result under r.autocheck
  const ac = (r.autocheck ?? r) as Record<string, unknown>;
  if (typeof ac.success !== "boolean") return null;
  const mapItems = (arr: unknown) =>
    Array.isArray(arr)
      ? (arr as Array<Record<string, unknown>>).map((w) => ({
          file: String(w.file ?? ""),
          line: Number(w.line ?? 0),
          message: String(w.message ?? ""),
        }))
      : [];
  return {
    success: ac.success,
    warnings: mapItems(ac.warnings),
    errors: mapItems(ac.errors),
  };
}

export function AutocheckDetails({
  warnings,
  errors,
}: {
  warnings: Array<{ file: string; line: number; message: string }>;
  errors: Array<{ file: string; line: number; message: string }>;
}) {
  const items = [
    ...errors.map((e) => ({ ...e, kind: "error" as const })),
    ...warnings.map((w) => ({ ...w, kind: "warning" as const })),
  ];
  if (items.length === 0) return null;
  return (
    <div className={styles.autocheckList}>
      {items.map((item, i) => (
        <div key={i} className={styles.autocheckItem}>
          <span className={item.kind === "error" ? styles.badgeErr : styles.badgeWarn}
            style={{ fontSize: 10, padding: "1px 5px" }}>
            {item.kind === "error" ? "error" : "warn"}
          </span>
          <span className={styles.autocheckLoc}>
            {item.file}:{item.line}
          </span>
          <span className={styles.autocheckMsg}>{item.message}</span>
        </div>
      ))}
    </div>
  );
}
