import { useState, useEffect } from "react";
import type { ToolBubble } from "../api/types";
import { ToolHeader, ToolBodyWrap } from "./ToolShared";
import sharedStyles from "./ToolShared.module.css";
import styles from "./PlanTool.module.css";

// ── types ──────────────────────────────────────────────────────────────────────

type StepStatus = "pending" | "in_progress" | "completed" | "skipped";
type StepPriority = "high" | "medium" | "low";

interface PlanStep {
  id: string;
  content: string;
  status: StepStatus;
  priority?: StepPriority;
}

// ── icons ──────────────────────────────────────────────────────────────────────

function ListIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
      <line x1="5" y1="4" x2="13" y2="4" />
      <line x1="5" y1="8" x2="13" y2="8" />
      <line x1="5" y1="12" x2="13" y2="12" />
      <circle cx="2.5" cy="4" r="0.8" fill="currentColor" stroke="none" />
      <circle cx="2.5" cy="8" r="0.8" fill="currentColor" stroke="none" />
      <circle cx="2.5" cy="12" r="0.8" fill="currentColor" stroke="none" />
    </svg>
  );
}

function StatusIcon({ status }: { status: StepStatus }) {
  if (status === "completed") {
    return (
      <svg className={styles.statusIconCompleted} width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
        <circle cx="8" cy="8" r="7" fill="currentColor" opacity="0.15" stroke="currentColor" strokeWidth="1.2" />
        <polyline points="5,8.5 7,10.5 11,6" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    );
  }
  if (status === "in_progress") {
    return (
      <svg className={styles.statusIconActive} width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
        <circle cx="8" cy="8" r="7" stroke="currentColor" strokeWidth="1.2" />
        <circle cx="8" cy="8" r="3" fill="currentColor" />
      </svg>
    );
  }
  if (status === "skipped") {
    return (
      <svg className={styles.statusIconSkipped} width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
        <circle cx="8" cy="8" r="7" stroke="currentColor" strokeWidth="1.2" strokeDasharray="3 2" />
        <line x1="5.5" y1="5.5" x2="10.5" y2="10.5" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
      </svg>
    );
  }
  // pending
  return (
    <svg className={styles.statusIconPending} width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <circle cx="8" cy="8" r="7" stroke="currentColor" strokeWidth="1.2" />
    </svg>
  );
}

// ── helpers ────────────────────────────────────────────────────────────────────

function parsePlanResult(result: unknown): { title: string; steps: PlanStep[] } | null {
  if (!result || typeof result !== "object") return null;
  const r = result as Record<string, unknown>;
  let obj: Record<string, unknown> = r;
  if (typeof r.text === "string") {
    try { obj = JSON.parse(r.text) as Record<string, unknown>; } catch { return null; }
  }
  if (obj.display !== "plan") return null;
  const title = typeof obj.title === "string" ? obj.title : "执行计划";
  const steps: PlanStep[] = Array.isArray(obj.steps)
    ? (obj.steps as Array<Record<string, unknown>>).map((s) => ({
        id: String(s.id ?? ""),
        content: String(s.content ?? ""),
        status: (s.status as StepStatus) ?? "pending",
        priority: s.priority != null ? (s.priority as StepPriority) : undefined,
      }))
    : [];
  return { title, steps };
}

function countByStatus(steps: PlanStep[]) {
  return {
    completed: steps.filter((s) => s.status === "completed").length,
    in_progress: steps.filter((s) => s.status === "in_progress").length,
    total: steps.length,
  };
}

// ── component ──────────────────────────────────────────────────────────────────

export function PlanTool({ bubble }: { bubble: ToolBubble }) {
  const [expanded, setExpanded] = useState(true);

  useEffect(() => {
    if (bubble.pending) setExpanded(true);
  }, [bubble.pending]);

  const plan = parsePlanResult(bubble.result);
  const label = bubble.description || (plan?.title ?? "制定计划");

  const badge = bubble.pending ? null : plan ? (() => {
    const { completed, in_progress, total } = countByStatus(plan.steps);
    if (total === 0) return null;
    if (completed === total) {
      return <span className={sharedStyles.badgeOk}>全部完成</span>;
    }
    if (in_progress > 0) {
      return (
        <span className={sharedStyles.badgeWarn}>
          {completed}/{total}
        </span>
      );
    }
    return (
      <span className={sharedStyles.badgePending}>
        {completed}/{total}
      </span>
    );
  })() : null;

  return (
    <div>
      <ToolHeader
        pending={bubble.pending}
        label={label}
        badges={badge}
        expanded={expanded}
        onToggle={() => setExpanded((v) => !v)}
        icon={<ListIcon />}
      />
      {expanded && (
        <ToolBodyWrap>
          {bubble.pending && !plan && (
            <div className={styles.pendingHint}>计划生成中…</div>
          )}
          {plan && plan.steps.length > 0 && (
            <ul className={styles.stepList}>
              {plan.steps.map((step) => (
                <li key={step.id} className={`${styles.step} ${styles[`step_${step.status}`]}`}>
                  <span className={styles.stepIcon}>
                    <StatusIcon status={step.status} />
                  </span>
                  <span className={styles.stepContent}>{step.content}</span>
                  {step.priority === "high" && step.status !== "completed" && (
                    <span className={styles.priorityDot} title="高优先级" />
                  )}
                </li>
              ))}
            </ul>
          )}
        </ToolBodyWrap>
      )}
    </div>
  );
}
