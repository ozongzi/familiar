import { useState, useEffect, useRef } from "react";
import { api } from "../api/client";
import { useAuth } from "../store/auth.shared";
import Chart from "chart.js/auto";
import styles from "./TokenUsageView.module.css";

type UserRow = {
  user_id: string;
  username: string;
  conversation_count: number;
  prompt_tokens: number;
  completion_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  total_tokens: number;
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
  total_tokens: number;
};

type DayRow = {
  day: string;
  total_tokens: number;
  prompt_tokens: number;
  completion_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  conversation_count: number;
};

type Summary = {
  prompt_tokens: number;
  completion_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  total_tokens: number;
  conversation_count: number;
};

function fmt(n: number): string {
  return n.toLocaleString();
}

export function TokenUsageView() {
  const { token } = useAuth();
  const [summary, setSummary] = useState<Summary | null>(null);
  const [users, setUsers] = useState<UserRow[]>([]);
  const [convs, setConvs] = useState<ConvRow[]>([]);
  const [days, setDays] = useState<DayRow[]>([]);
  const [selectedUserId, setSelectedUserId] = useState<string>("");
  const chartRef = useRef<HTMLCanvasElement>(null);
  const chartInstance = useRef<Chart | null>(null);

  useEffect(() => {
    if (!token) return;
    api.getTokenUsage(token).then(setSummary).catch(() => {});
    api.getTokenUsageByUser(token).then(r => setUsers(r.users)).catch(() => {});
    api.getTokenUsageDaily(token).then(r => setDays(r.days)).catch(() => {});
  }, [token]);

  useEffect(() => {
    if (!token) return;
    api.getTokenUsageConversations(token, selectedUserId || undefined)
      .then(r => setConvs(r.conversations))
      .catch(() => {});
  }, [token, selectedUserId]);

  useEffect(() => {
    if (!chartRef.current || days.length === 0) return;
    if (chartInstance.current) chartInstance.current.destroy();
    chartInstance.current = new Chart(chartRef.current, {
      type: "bar",
      data: {
        labels: days.map(d => d.day),
        datasets: [
          {
            label: "Prompt",
            data: days.map(d => d.prompt_tokens),
            backgroundColor: "rgba(99,102,241,0.7)",
            stack: "tokens",
          },
          {
            label: "Completion",
            data: days.map(d => d.completion_tokens),
            backgroundColor: "rgba(34,197,94,0.7)",
            stack: "tokens",
          },
          {
            label: "Cache Read",
            data: days.map(d => d.cache_read_tokens),
            backgroundColor: "rgba(59,130,246,0.5)",
            stack: "tokens",
          },
          {
            label: "Cache Creation",
            data: days.map(d => d.cache_creation_tokens),
            backgroundColor: "rgba(251,191,36,0.5)",
            stack: "tokens",
          },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        plugins: {
          legend: { position: "top" },
          title: { display: true, text: "近 30 天 Token 用量" },
        },
        scales: {
          x: { stacked: true },
          y: { stacked: true },
        },
      },
    });
    return () => { chartInstance.current?.destroy(); };
  }, [days]);

  return (
    <div className={styles.root}>
      {/* 汇总卡片 */}
      {summary && (
        <div className={styles.summaryRow}>
          {([
            ["对话数", summary.conversation_count],
            ["总 Token", summary.total_tokens],
            ["Prompt", summary.prompt_tokens],
            ["Completion", summary.completion_tokens],
            ["Cache Read", summary.cache_read_tokens],
            ["Cache Creation", summary.cache_creation_tokens],
          ] as [string, number][]).map(([label, val]) => (
            <div key={label} className={styles.card}>
              <span className={styles.cardLabel}>{label}</span>
              <strong className={styles.cardValue}>{fmt(val)}</strong>
            </div>
          ))}
        </div>
      )}

      {/* 趋势图 */}
      <div className={styles.chartWrap}>
        <canvas ref={chartRef} />
      </div>

      {/* 用户列表 */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>按用户统计</h3>
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
            {users.map(u => (
              <tr key={u.user_id} className={selectedUserId === u.user_id ? styles.rowSelected : ""}>
                <td>{u.username}</td>
                <td>{fmt(u.conversation_count)}</td>
                <td>{fmt(u.prompt_tokens)}</td>
                <td>{fmt(u.completion_tokens)}</td>
                <td>{fmt(u.cache_read_tokens)} / {fmt(u.cache_creation_tokens)}</td>
                <td><strong>{fmt(u.total_tokens)}</strong></td>
                <td>
                  <button
                    className={styles.filterBtn}
                    onClick={() => setSelectedUserId(prev => prev === u.user_id ? "" : u.user_id)}
                  >
                    {selectedUserId === u.user_id ? "取消筛选" : "查看对话"}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>

      {/* Conversation 明细 */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>
          对话明细
          {selectedUserId && users.find(u => u.user_id === selectedUserId) && (
            <span className={styles.filterTag}>
              {users.find(u => u.user_id === selectedUserId)!.username}
            </span>
          )}
        </h3>
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
            {convs.map(c => (
              <tr key={c.conv_id}>
                <td className={styles.convName}>{c.conv_name}</td>
                <td>{c.username}</td>
                <td className={styles.date}>{c.created_at.slice(0, 10)}</td>
                <td>{fmt(c.prompt_tokens)}</td>
                <td>{fmt(c.completion_tokens)}</td>
                <td>{fmt(c.cache_read_tokens)} / {fmt(c.cache_creation_tokens)}</td>
                <td><strong>{fmt(c.total_tokens)}</strong></td>
              </tr>
            ))}
            {convs.length === 0 && (
              <tr><td colSpan={7} className={styles.empty}>暂无数据</td></tr>
            )}
          </tbody>
        </table>
      </section>
    </div>
  );
}
