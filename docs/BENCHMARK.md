# Alice benchmark (darwin, 30 turns, fake LLM first-token delay 50 ms)

| Metric | Value |
|---|---|
| CLI: `alice --version` (median) | 4.2 ms |
| CLI: `alice chat list (API round trip)` (median) | 4.0 ms |
| CLI: `alice status (API round trip)` (median) | 5.5 ms |
| Idle RSS after start | 11.0 MB |
| Idle RSS after 30 turns | 13.6 MB |
| Alice overhead per turn (context + memory search + DB), median | 0.30 ms |
| Provider time per turn (fake LLM), median | 54.0 ms |
| Provider time to first token, median | 54.0 ms |
| Client wall time per turn incl. polling, median | 60.3 ms |
| Alice overhead per turn, max | 0.97 ms |
| Database size after 30 turns (incl. WAL) | 929 KB |

Measured 2026-11-24 / macOS 27.2, Apple Silicon / release build (binary 9.3 MB)
Reproduce: `cargo build --release && scripts/bench.py target/release/alice 30`

## Against the goals

| Goal | Result |
|---|---|
| CLI startup under 10 ms | Met (`--version` about 4 ms; commands with an API round trip 4–6 ms) |
| Idle memory under 20 MB | Met (about 11 MB after start, about 14 MB after 30 turns) |
| Alice's own processing time | 0.3 ms median per turn (context, memory search, DB writes). Nearly all of the response time is the model's |
| No needless background work | No timers while idle except the 20 s SSE keep-alive per connection, the scheduler's 20 s tick, and long polling / heartbeats of enabled gateways |
| Fewer needless model calls | Thread titles are made without a model. Memory extraction runs once per turn and skips short messages. Screen comments are cached |

## Notes
- "Client wall time" includes the benchmark's 5 ms polling interval. Real clients get events at once over SSE.
- The DB size includes the WAL, which is truncated when threads are deleted.
- Linux (x86_64 / ARM64) has not been benchmarked; CI runs the same tests there.
