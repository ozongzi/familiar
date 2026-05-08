import { useEffect, useRef, useState } from "react";
import { api } from "../api/client";
import Chart from "chart.js/auto";
import type { AdminView } from "../pages/AdminPage";
import styles from "./AdminOverview.module.css";

interface Props {
  token: string;
  onNavigate: (view: AdminView) => void;
}

type Summary = {
  prompt_tokens: number;
  completion_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  cost_input: number;
  cost_output: number;
  cost_cache_read: number;
  cost_cache_creation: number;
  total_cost: number;
  conversation_count: number;
};

type DayRow = {
  day: string;
  prompt_tokens: number;
  completion_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  cost_input: number;
  cost_output: number;
  cost_cache_read: number;
  cost_cache_creation: number;
  total_cost: number;
  conversation_count: number;
};

type UserRow = {
  user_id: string;
  username: string;
  conversation_count: number;
  prompt_tokens: number;
  completion_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  total_cost: number;
};

type ConvRow = {
  conv_id: string;
  conv_name: string;
  username: string;
  created_at: string;
  prompt_tokens: number;
  completion_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  total_cost: number;
};

function fmt(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "K";
  return String(n);
}

function fmtUsd(n: number): string {
  if (!Number.isFinite(n)) return "$0.00";
  if (n >= 1_000) return "$" + (n / 1_000).toFixed(2) + "K";
  return "$" + n.toFixed(2);
}

export function AdminOverview({ token, onNavigate }: Props) {
  const [summary, setSummary] = useState<Summary | null>(null);
  const [days, setDays] = useState<DayRow[]>([]);
  const [users, setUsers] = useState<UserRow[]>([]);
  const [convs, setConvs] = useState<ConvRow[]>([]);
  const [selectedUserId, setSelectedUserId] = useState("");
  const [modelCount, setModelCount] = useState(0);
  const chartRef = useRef<HTMLCanvasElement>(null);
  const chartInstance = useRef<Chart | null>(null);

  useEffect(() => {
    api.getTokenUsage(token).then(setSummary).catch(() => {});
    api.getTokenUsageDaily(token).then((r) => setDays(r.days)).catch(() => {});
    api.getTokenUsageByUser(token).then((r) => setUsers(r.users)).catch(() => {});
    api.adminListModels(token).then((ms) => setModelCount(ms.length)).catch(() => {});
  }, [token]);

  useEffect(() => {
    api.getTokenUsageConversations(token, selectedUserId || undefined)
      .then((r) => setConvs(r.conversations))
      .catch(() => {});
  }, [token, selectedUserId]);

  useEffect(() => {
    if (!chartRef.current || days.length === 0) return;
    if (chartInstance.current) chartInstance.current.destroy();
    chartInstance.current = new Chart(chartRef.current, {
      type: "bar",
      data: {
        labels: days.map((d) => d.day.slice(5)),
        datasets: [
          { label: "Input", data: days.map((d) => d.cost_input), backgroundColor: "rgba(99,102,241,0.6)", stack: "t" },
          { label: "Output", data: days.map((d) => d.cost_output), backgroundColor: "rgba(34,197,94,0.6)", stack: "t" },
          { label: "Cache Read", data: days.map((d) => d.cost_cache_read), backgroundColor: "rgba(59,130,246,0.45)", stack: "t" },
          { label: "Cache Write", data: days.map((d) => d.cost_cache_creation), backgroundColor: "rgba(251,191,36,0.45)", stack: "t" },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        plugins: {
          legend: { position: "top" },
          tooltip: {
            callbacks: { label: (ctx) => `${ctx.dataset.label}: ${fmtUsd(Number(ctx.parsed.y))}` },
          },
        },
        scales: {
          x: { stacked: true, grid: { display: false }, ticks: { font: { size: 11 } } },
          y: {
            stacked: true,
            grid: { color: "rgba(128,128,128,0.1)" },
            ticks: { font: { size: 11 }, callback: (v) => fmtUsd(Number(v)) },
          },
        },
      },
    });
    return () => { chartInstance.current?.destroy(); };
  }, [days]);

  const todayCost = days[days.length - 1]?.total_cost ?? 0;

  return (
    <div className={styles.root}>
      {/* Stat cards */}
      <div className={styles.cards}>
        <StatCard label="总费用" value={fmtUsd(summary?.total_cost ?? 0)} sub="累计 USD" />
        <StatCard label="今日费用" value={fmtUsd(todayCost)} sub="过去24h USD" />
        <StatCard label="对话总数" value={fmt(summary?.conversation_count ?? 0)} sub="累计" />
        <StatCard label="Cache Read" value={fmt(summary?.cache_read_tokens ?? 0)} sub="tokens" />
        <StatCard label="Cache Creation" value={fmt(summary?.cache_creation_tokens ?? 0)} sub="tokens" />
        <StatCard label="全局模型" value={String(modelCount)} sub="已配置" onClick={() => onNavigate("models")} />
      </div>

      {/* Chart */}
      <div className={styles.chartCard}>
        <div className={styles.cardHeader}>费用趋势（近30天，USD）</div>
        <div className={styles.chartWrap}>
          <canvas ref={chartRef} />
        </div>
      </div>

      {/* Users + convs */}
      <div className={styles.row}>
        <div className={styles.tableCard}>
          <div className={styles.cardHeader}>按用户统计</div>
          <table className={styles.table}>
            <thead>
              <tr>
                <th>用户</th>
                <th>对话数</th>
                <th>Prompt</th>
                <th>Completion</th>
                <th>Cache R/C</th>
                <th>总计</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {users.map((u) => (
                <tr key={u.user_id} className={selectedUserId === u.user_id ? styles.rowSelected : ""}>
                  <td className={styles.username}>{u.username}</td>
                  <td>{u.conversation_count}</td>
                  <td>{fmt(u.prompt_tokens)}</td>
                  <td>{fmt(u.completion_tokens)}</td>
                  <td>{fmt(u.cache_read_tokens)} / {fmt(u.cache_creation_tokens)}</td>
                  <td><strong>{fmtUsd(u.total_cost)}</strong></td>
                  <td>
                    <button className={styles.filterBtn}
                      onClick={() => setSelectedUserId((p) => p === u.user_id ? "" : u.user_id)}>
                      {selectedUserId === u.user_id ? "取消" : "筛选"}
                    </button>
                  </td>
                </tr>
              ))}
              {users.length === 0 && <tr><td colSpan={7} className={styles.empty}>暂无数据</td></tr>}
            </tbody>
          </table>
        </div>

        <div className={styles.tableCard}>
          <div className={styles.cardHeader}>
            对话明细
            {selectedUserId && users.find((u) => u.user_id === selectedUserId) && (
              <span className={styles.filterTag}>
                {users.find((u) => u.user_id === selectedUserId)!.username}
              </span>
            )}
          </div>
          <table className={styles.table}>
            <thead>
              <tr>
                <th>对话名</th>
                <th>用户</th>
                <th>时间</th>
                <th>Prompt</th>
                <th>Completion</th>
                <th>Cache R/C</th>
                <th>总计</th>
              </tr>
            </thead>
            <tbody>
              {convs.map((c) => (
                <tr key={c.conv_id}>
                  <td className={styles.username}>{c.conv_name}</td>
                  <td>{c.username}</td>
                  <td className={styles.date}>{c.created_at.slice(0, 10)}</td>
                  <td>{fmt(c.prompt_tokens)}</td>
                  <td>{fmt(c.completion_tokens)}</td>
                  <td>{fmt(c.cache_read_tokens)} / {fmt(c.cache_creation_tokens)}</td>
                  <td><strong>{fmtUsd(c.total_cost)}</strong></td>
                </tr>
              ))}
              {convs.length === 0 && <tr><td colSpan={7} className={styles.empty}>暂无数据</td></tr>}
            </tbody>
          </table>
        </div>
      </div>

      {/* Quick actions */}
      <div className={styles.actionsCard}>
        <div className={styles.cardHeader}>快捷操作</div>
        <div className={styles.actionBtns}>
          <ActionBtn label="添加全局模型" onClick={() => onNavigate("models")} />
          <ActionBtn label="管理用户" onClick={() => onNavigate("users")} />
          <ActionBtn label="系统配置" onClick={() => onNavigate("config")} />
          <ActionBtn label="审计日志" onClick={() => onNavigate("audit")} />
        </div>
      </div>
    </div>
  );
}

function StatCard({ label, value, sub, onClick }: { label: string; value: string; sub: string; onClick?: () => void }) {
  return (
    <div className={`${styles.statCard} ${onClick ? styles.statCardClickable : ""}`} onClick={onClick}>
      <div className={styles.statValue}>{value}</div>
      <div className={styles.statLabel}>{label}</div>
      <div className={styles.statSub}>{sub}</div>
    </div>
  );
}

function ActionBtn({ label, onClick }: { label: string; onClick: () => void }) {
  return <button className={styles.actionBtn} onClick={onClick}>{label}</button>;
}
