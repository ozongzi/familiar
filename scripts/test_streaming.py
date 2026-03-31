#!/usr/bin/env python3
"""
Test whether familiar's SSE streaming is truly streaming or batched.

Usage:
    python3 scripts/test_streaming.py --url https://familiar.fhmmt.games --token <session_token> --conv <conv_id>

What it measures:
  - Time from sending the message to receiving the FIRST token  (TTFT)
  - Inter-token delay distribution (p50 / p95 / max)
  - Whether tokens arrive in tight bursts (batched) or spread out (true streaming)

A "fake" stream shows:
  - Very long TTFT (several seconds)
  - Then many tokens arriving < 5ms apart (they were buffered and flushed at once)

A "real" stream shows:
  - Moderate TTFT (model latency)
  - Tokens arriving 20-150ms apart (real LLM generation cadence)
"""

import argparse
import json
import sys
import time
import urllib.request
import urllib.error
import uuid

def send_message(base_url: str, token: str, conv_id: str, text: str) -> str:
    """POST a message and return the job_id (from the response body)."""
    url = f"{base_url}/api/conversations/{conv_id}/messages"
    body = json.dumps({"content": text, "images": []}).encode()
    req = urllib.request.Request(
        url,
        data=body,
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {token}",
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=15) as resp:
        data = json.loads(resp.read())
        return data.get("stream_id") or data.get("job_id") or data.get("id") or ""


def stream_sse(base_url: str, token: str, conv_id: str, job_id: str):
    """Open the SSE stream and yield (timestamp, event_type, content) tuples."""
    url = f"{base_url}/api/stream/{job_id}"
    req = urllib.request.Request(
        url,
        headers={
            "Accept": "text/event-stream",
            "Authorization": f"Bearer {token}",
        },
    )
    with urllib.request.urlopen(req, timeout=120) as resp:
        for raw_line in resp:
            line = raw_line.decode("utf-8").rstrip("\n\r")
            if not line.startswith("data:"):
                continue
            payload = line[5:].strip()
            try:
                ev = json.loads(payload)
            except json.JSONDecodeError:
                continue
            yield time.monotonic(), ev.get("type", ""), ev.get("content", "")
            if ev.get("type") in ("done", "aborted", "error"):
                break


def analyse(base_url: str, token: str, conv_id: str, text: str):
    print(f"→ Sending: {text!r}")
    t_send = time.monotonic()

    job_id = send_message(base_url, token, conv_id, text)
    print(f"  job_id: {job_id}")

    token_times: list[float] = []
    burst_gaps: list[float] = []  # gaps < 10ms indicate batching

    print("\n  Receiving tokens...")
    for ts, ev_type, content in stream_sse(base_url, token, conv_id, job_id):
        if ev_type == "token":
            token_times.append(ts)
            if len(token_times) >= 2:
                gap_ms = (token_times[-1] - token_times[-2]) * 1000
                burst_gaps.append(gap_ms)
                # Print first 20 tokens with timing
                if len(token_times) <= 20:
                    print(f"    [{len(token_times):3d}] +{gap_ms:6.1f}ms  {content!r}")
            else:
                ttft = (ts - t_send) * 1000
                print(f"    [  1] TTFT={ttft:.0f}ms  {content!r}")

    if not token_times:
        print("  ERROR: no tokens received")
        return

    total = len(token_times)
    total_dur_ms = (token_times[-1] - token_times[0]) * 1000
    ttft_ms = (token_times[0] - t_send) * 1000

    if burst_gaps:
        burst_gaps.sort()
        p50 = burst_gaps[len(burst_gaps) // 2]
        p95 = burst_gaps[int(len(burst_gaps) * 0.95)]
        pmax = burst_gaps[-1]
        sub5 = sum(1 for g in burst_gaps if g < 5)
        burst_pct = sub5 / len(burst_gaps) * 100
    else:
        p50 = p95 = pmax = burst_pct = 0

    print(f"\n{'='*50}")
    print(f"  Tokens received : {total}")
    print(f"  TTFT            : {ttft_ms:.0f} ms")
    print(f"  Total stream dur: {total_dur_ms:.0f} ms")
    print(f"  Inter-token p50 : {p50:.1f} ms")
    print(f"  Inter-token p95 : {p95:.1f} ms")
    print(f"  Inter-token max : {pmax:.1f} ms")
    print(f"  Bursts (<5ms)   : {sub5}/{len(burst_gaps)} = {burst_pct:.0f}%")
    print()

    if burst_pct > 50:
        print("  ⚠️  VERDICT: FAKE STREAMING — >50% of tokens arrive in bursts (<5ms apart)")
        print("     Tokens are being buffered (likely by DB notify batching or buffered HTTP)")
    elif p50 < 10 and burst_pct > 20:
        print("  ⚠️  VERDICT: PARTIAL BATCHING — tokens arrive in clusters")
    else:
        print("  ✅  VERDICT: TRUE STREAMING — tokens spread out naturally")
    print('='*50)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", default="https://familiar.fhmmt.games")
    parser.add_argument("--token", required=True, help="Session cookie value")
    parser.add_argument("--conv", required=True, help="Conversation UUID")
    parser.add_argument("--text", default="请用中文数到二十，每个数字单独一行。")
    args = parser.parse_args()

    analyse(args.url, args.token, args.conv, args.text)


if __name__ == "__main__":
    main()
