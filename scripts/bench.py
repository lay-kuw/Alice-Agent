#!/usr/bin/env python3
"""Measure Alice's own overhead, separately from LLM latency.

Starts a local fake OpenAI-compatible LLM with a fixed delay, runs
`alice serve` in a temporary home, and reports:
  - CLI startup time
  - idle memory (RSS) before and after conversations
  - per-turn Alice overhead (context build + DB) vs. provider time
  - time from LLM first token to the client receiving it

Usage: scripts/bench.py [path/to/alice] [turns]
Needs only the Python standard library.
"""
import http.server, json, os, statistics, subprocess, sys, tempfile, threading, time, urllib.request

ALICE = sys.argv[1] if len(sys.argv) > 1 else "target/release/alice"
TURNS = int(sys.argv[2]) if len(sys.argv) > 2 else 30
LLM_DELAY = 0.05  # seconds before the first token, simulating a fast provider
LLM_PORT, ALICE_PORT = 18751, 18752


class FakeLLM(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        system = body["messages"][0]["content"] if body["messages"][0]["role"] == "system" else ""
        text = '{"memories":[]}' if "long-term memory" in system else "This is a benchmark reply. " * 8
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.end_headers()
        time.sleep(LLM_DELAY)
        for i in range(0, len(text), 16):
            chunk = {"choices": [{"index": 0, "delta": {"content": text[i:i + 16]}}]}
            self.wfile.write(f"data: {json.dumps(chunk)}\n\n".encode())
            self.wfile.flush()
        self.wfile.write(b"data: [DONE]\n\n")


def median_ms(cmd, env, n=30):
    ts = []
    for _ in range(n):
        t = time.perf_counter()
        subprocess.run(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        ts.append((time.perf_counter() - t) * 1000)
    return statistics.median(ts)


def rss_mb(pid):
    out = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
    return int(out) / 1024 if out else 0.0


def main():
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", LLM_PORT), FakeLLM)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    home = tempfile.mkdtemp(prefix="alice-bench-")
    env = dict(os.environ, ALICE_HOME=home)
    with open(os.path.join(home, "config.toml"), "w") as f:
        f.write(f"""[server]\nport = {ALICE_PORT}\n[agent]\ndefault_model = "fake/m"\n
[[providers]]\nid = "fake"\nkind = "openai-compatible"\nbase_url = "http://127.0.0.1:{LLM_PORT}/v1"\nmodels = ["m"]\n""")
    token = "bench-token-0123456789"
    with open(os.path.join(home, "secrets.toml"), "w") as f:
        f.write(f'"server.token" = "{token}"\n')
    server = subprocess.Popen([ALICE, "serve"], env=env, stderr=subprocess.DEVNULL)
    base = f"http://127.0.0.1:{ALICE_PORT}/api"

    def api(method, path, body=None):
        req = urllib.request.Request(base + path, method=method, data=json.dumps(body).encode() if body is not None else None,
                                     headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"})
        with urllib.request.urlopen(req) as r:
            data = r.read()
            return json.loads(data) if data else None

    try:
        for _ in range(100):
            try:
                api("GET", "/health")
                break
            except Exception:
                time.sleep(0.05)
        time.sleep(0.5)
        idle_start = rss_mb(server.pid)
        cli = {
            "alice --version": median_ms([ALICE, "--version"], env),
            "alice chat list (API round trip)": median_ms([ALICE, "chat", "list"], env),
            "alice status (API round trip)": median_ms([ALICE, "status"], env),
        }
        thread = api("POST", "/threads", {})["thread"]["id"]
        alice_ms, provider_ms, ttft_ms, wall_ms = [], [], [], []
        for i in range(TURNS):
            t0 = time.perf_counter()
            r = api("POST", f"/threads/{thread}/messages", {"content": f"benchmark message number {i} about my favourite topics"})
            mid = r["assistant_message"]["id"]
            while True:
                msgs = api("GET", f"/threads/{thread}/messages")["messages"]
                m = next(x for x in msgs if x["id"] == mid)
                if m["status"] != "streaming":
                    break
                time.sleep(0.005)
            wall_ms.append((time.perf_counter() - t0) * 1000)
            alice_ms.append(m["meta"]["alice_us"] / 1000)
            provider_ms.append(m["meta"]["provider_ms"])
            ttft_ms.append(m["meta"]["ttft_ms"] or 0)
            time.sleep(0.02)  # let memory extraction finish
        time.sleep(0.5)
        idle_end = rss_mb(server.pid)
        db = sum(os.path.getsize(os.path.join(home, f)) for f in os.listdir(home) if f.startswith("alice.db"))
        print(f"# Alice benchmark ({sys.platform}, {TURNS} turns, fake LLM first-token delay {LLM_DELAY * 1000:.0f} ms)\n")
        print("| Metric | Value |\n|---|---|")
        for k, v in cli.items():
            print(f"| CLI: `{k}` (median) | {v:.1f} ms |")
        print(f"| Idle RSS after start | {idle_start:.1f} MB |")
        print(f"| Idle RSS after {TURNS} turns | {idle_end:.1f} MB |")
        print(f"| Alice overhead per turn (context + memory search + DB), median | {statistics.median(alice_ms):.2f} ms |")
        print(f"| Provider time per turn (fake LLM), median | {statistics.median(provider_ms):.1f} ms |")
        print(f"| Provider time to first token, median | {statistics.median(ttft_ms):.1f} ms |")
        print(f"| Client wall time per turn incl. polling, median | {statistics.median(wall_ms):.1f} ms |")
        print(f"| Alice overhead per turn, max | {max(alice_ms):.2f} ms |")
        print(f"| Database size after {TURNS} turns (incl. WAL) | {db / 1024:.0f} KB |")
    finally:
        server.terminate()
        server.wait()
        srv.shutdown()


if __name__ == "__main__":
    main()
