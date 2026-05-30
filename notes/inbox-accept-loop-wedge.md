# Theater accept loop wedges over time (observed on inbox VPS)

## Symptoms

After running for some time (days, not consistently measured), a theater
process serving inbox stops accepting new TCP connections on its HTTPS
listener (port 443) — but the process is otherwise alive:

- `systemctl status` reports `active (running)`
- `ss -tlnp` shows the listener in LISTEN state, but with a non-zero (and
  often very high) `Recv-Q` — pending TCP connections queued in the
  kernel, never being `accept()`'d by theater
- The process is `S (sleeping)`, low CPU, plenty of file descriptors
  available, plenty of memory
- Other listeners (SMTP on :25) may still partially work — we've seen
  smtp session errors continue to be logged while HTTPS is wedged
- External probes get `connect timed out`, not `connection refused`
  (because the TCP SYN gets through, the connection sits in the backlog)

## Diagnostic signal that's *correlated* with the wedge but not proven causal

`/var/log/inbox/theater.log` floods with:

```
WARN  Failed to send event notification: no available capacity
```

emitted from `crates/theater/src/chain/mod.rs` (around the
`theater_tx.try_send(TheaterCommand::NewEvent { ... })` call). The
`theater_tx` channel reaches capacity and `try_send` returns an error,
which we log and drop. We have not confirmed this is what's blocking
the accept loop — it could be a parallel symptom of the same root cause
rather than the cause itself.

## What we know

- Restarting `inbox.service` clears it completely (Recv-Q drops to 0,
  connections accepted normally)
- Observed twice: between 2026-05-19 and 2026-05-20, and again between
  2026-05-20 and 2026-05-22

## What we don't know

- Whether the dropped-event warning is the *cause* of the accept-loop
  wedge or a symptom of some other backpressure
- Whether the wedge is timing-related (event channel fills under load
  → accept loop blocks waiting on the same lock?) or some other
  resource exhaustion
- Whether it only happens under specific traffic patterns (many
  per-connection actors spawning rapidly, e.g. SMTP storms?)

## Suggested next steps when this recurs

1. Before restarting: dump `/proc/<pid>/stack` for each theater thread
   (need root) — would show exactly where each thread is blocked
2. Capture `ss -tlnp` (queue depth) + `ps -L -p <pid>` (thread state) +
   theater log tail at the moment of wedge
3. Then check: is the accept thread blocked waiting on the same lock /
   channel as the event-broadcast machinery?

## Quick mitigation ideas (untested)

- Bump the `theater_tx` channel capacity (cheap, may just delay)
- Use `tokio::sync::broadcast` (overwrites old slots when full) for
  event notifications instead of an mpsc with `try_send`
- Move event notification off the chain-append hot path (queue
  separately, batched by a background task)

None of these address the root cause, only the symptom we've observed.

## Recurrence #5 — 2026-05-30 ~16:22 UTC

First wedge observed with PR #67 (accept-loop `tokio::spawn`) +
PR #76 (close_notify active-mode) + PR #80 (close_notify
cleanup-clears-shared-map) all deployed.

- **Uptime at detection:** 1d 2h ~33m (service started 2026-05-29
  13:48 UTC after the prior restart)
- **Deployed binary:** `/nix/store/am16fdw66xzmklsf8ni9czfbifjmxwmf-theater-0.3.9`
  (should include #67 / #76 / #80)
- **Fingerprint:** identical to the original pre-#67 wedge
    - `Recv-Q` 129/128 on :443
    - 43 `CLOSE-WAIT`, 36 `ESTAB` on :443
    - Per-thread: main + 1 worker `futex_wait`, 1 worker
      `epoll_wait` (all `wchan=0` from /proc, no kernel-side block)
    - 3 threads total, 38 FDs (normal)
    - Chain logs last updated 15:55 UTC (27 min before detection)
    - Journal silent since restart (no errors)
- **System resources fine:** disk 76%, inodes 39%, no swap
  pressure (mem 526MB/964MB used, 93MB free, 343MB buff/cache)
- **Side observation (probably unrelated):** `/tmp/theater/chains`
  is 6.7GB across 80k files; largest single chain 800MB (some
  api-handler accumulated). Hygiene-relevant, not the immediate
  cause.

### Interpretation

PR #67's fix didn't prevent this recurrence. Possibilities:

  a) Fix shipped but is insufficient — there's another blocking
     point in the accept path beyond `handle-connection`.
  b) Fix didn't actually make it into `am16fdw66` (build
     mismatch — worth verifying via `strings`/symbols).
  c) Something downstream of #67 (post-deploy code) re-introduced
     the same blocking pattern.

Cadence is now ~26h between wedges with the fix in place, vs
~36h pre-fix per the earlier note.

Restart cleared it as expected. No deeper investigation per
Colin's call — fold into the test-infra Track 1 backlog
(real-network harness for accept-loop survival under load) when
that work picks up.
