# Roadmap: rs485-logger

## Milestones

- ✅ **v1.0 MVP** — Phases 1–7 (shipped 2026-04-03) — [archive](.planning/milestones/v1.0-ROADMAP.md)

## Phases

<details>
<summary>✅ v1.0 MVP (Phases 1–7) — SHIPPED 2026-04-03</summary>

- [x] Phase 1: Foundation (3/3 plans) — completed 2026-04-02
- [x] Phase 2: InfluxDB Integration (2/2 plans) — completed 2026-04-02
- [x] Phase 3: Modbus + Poll Loop (3/3 plans) — completed 2026-04-02
- [x] Phase 4: Systemd Deployment (2/2 plans) — completed 2026-04-02
- [x] Phase 5: README / Manual (1/1 plan) — completed 2026-04-02
- [x] Phase 6: Daily Energy Reset (2/2 plans) — completed 2026-04-03
- [x] Phase 7: Daemon Reliability Hardening (3/3 plans) — completed 2026-04-03

Full details: [.planning/milestones/v1.0-ROADMAP.md](milestones/v1.0-ROADMAP.md)

</details>

## Progress

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Foundation | v1.0 | 3/3 | Complete | 2026-04-02 |
| 2. InfluxDB Integration | v1.0 | 2/2 | Complete | 2026-04-02 |
| 3. Modbus + Poll Loop | v1.0 | 3/3 | Complete | 2026-04-02 |
| 4. Systemd Deployment | v1.0 | 2/2 | Complete | 2026-04-02 |
| 5. README / Manual | v1.0 | 1/1 | Complete | 2026-04-02 |
| 6. Daily Energy Reset | v1.0 | 2/2 | Complete | 2026-04-03 |
| 7. Daemon Reliability Hardening | v1.0 | 3/3 | Complete | 2026-04-03 |

### Phase 1: Research and find a way to poll devices faster when attach multiples rs485 in a chain. Now, I have 5 devices connected into rs485-usb, even though I set poll interval to 1 second, it takes 5 seconds to poll every devices.

**Goal:** Reduce 5-device poll cycle from ~3-5s to <1s by tuning read timeout (500ms → 150ms configurable), splitting inter-frame delay (30ms reads / 100ms resets), and adding cycle duration monitoring.
**Requirements**: D-01, D-02, D-03, D-04, D-05, D-06, D-07, D-08, D-09
**Depends on:** Phase 0
**Plans:** 1 plan

Plans:
- [x] 01-01-PLAN.md — Config + poller + main loop poll speed optimization

### Phase 2: Revisit polling slowlyness

**Goal:** Eliminate ~3.15s InfluxDB write bottleneck from poll loop via fire-and-forget tokio::spawn writes; fold in CFG-02 parity config and RISK-1 udev rule fix.
**Requirements**: D-01, D-02, D-03, D-04, D-05, D-06, D-07, D-08, D-09, D-10, D-11, D-12, D-13, D-14
**Depends on:** Phase 1
**Plans:** 2 plans

Plans:
- [ ] 02-01-PLAN.md — Decouple InfluxDB writes from poll loop (fire-and-forget + Arc<AtomicBool> health)
- [ ] 02-02-PLAN.md — Add parity config (CFG-02) + fix udev rule driver mismatch (RISK-1)
