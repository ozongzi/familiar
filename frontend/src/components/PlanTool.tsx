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


function parseStepFromArgs(raw: Record<string, unknown>): PlanStep | null {
  if (!raw || typeof raw.id === "undefined") return null;
  const content = typeof raw.content === "string" ? raw.content : "";
  if (!content) return null;
  return {
    id: String(raw.id),
    content,
    status: (raw.status as StepStatus) ?? "pending",
    priority: raw.priority != null ? (raw.priority as StepPriority) : undefined,
  };
}

function extractObjectsFromArray(arrayStr: string): PlanStep[] {
  const bracketPos = arrayStr.indexOf("[");
  if (bracketPos === -1) return [];

  const steps: PlanStep[] = [];
  let depth = 1;
  let objStart = -1;

  for (let i = bracketPos + 1; i < arrayStr.length; i++) {
    const ch = arrayStr[i];
    if (ch === "{") {
      if (depth === 1) objStart = i;
      depth++;
    } else if (ch === "}") {
      depth--;
      if (depth === 1 && objStart !== -1) {
        const objStr = arrayStr.slice(objStart, i + 1);
        try {
          const obj = JSON.parse(objStr) as Record<string, unknown>;
          const step = parseStepFromArgs(obj);
          if (step) steps.push(step);
        } catch {
          // incomplete object, skip
        }
        objStart = -1;
      }
      if (depth === 0) break;
    }
  }

  return steps;
}

function parseTodosFromArgs(argsRaw: string): PlanStep[] {
  if (!argsRaw) return [];

  // Try full JSON parse first — args shape: {title, steps: [...]}
  try {
    const parsed = JSON.parse(argsRaw) as Record<string, unknown>;
    if (Array.isArray(parsed.steps)) {
      return (parsed.steps as Array<Record<string, unknown>>)
        .map(parseStepFromArgs)
        .filter((s): s is PlanStep => s !== null);
    }
  } catch {
    // argsRaw not yet complete JSON, fall through to streaming parse
  }

  // Streaming parse: extract complete objects from the inline "steps" array
  const keyIdx = argsRaw.indexOf('"steps"');
  if (keyIdx === -1) return [];
  const bracketPos = argsRaw.indexOf("[", keyIdx);
  if (bracketPos === -1) return [];

  return extractObjectsFromArray(argsRaw.slice(bracketPos));
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

  const steps = parseTodosFromArgs(bubble.argsRaw);
  const label = bubble.description || "制定计划";

  const badge = bubble.pending ? null : (() => {
    const { completed, in_progress, total } = countByStatus(steps);
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
  })();

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
          {bubble.pending && steps.length === 0 && (
            <div className={styles.pendingHint}>计划生成中…</div>
          )}
          {steps.length > 0 && (
            <ul className={styles.stepList}>
              {steps.map((step) => (
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
