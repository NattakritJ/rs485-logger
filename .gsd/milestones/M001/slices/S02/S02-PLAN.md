# S02: InfluxDB Integration

**Status:** ✅ Completed 2026-04-02
**Goal:** Build the line protocol formatter and InfluxDB 3 HTTP write client; validate end-to-end write path against a local InfluxDB instance using hardcoded `PowerReading` values.
**Demo:** Integration test POSTs to a running local InfluxDB 3 instance and gets HTTP 204; `SHOW COLUMNS` confirms all fields are `DOUBLE` type (not INT64).

## Must-Haves

- `to_line_protocol()` — float-typed line protocol from PowerReading (TDD)
- `InfluxWriter` struct with reqwest HTTP POST + error handling + integration test
- Correct endpoint: `/api/v3/write_lp` with `Authorization: Bearer <token>`
- Field type safety: all values written as `f64` floats, never integers

## Tasks

- [x] T01: `to_line_protocol()` TDD — float-typed line protocol from PowerReading
- [x] T02: `InfluxWriter` struct with reqwest HTTP POST + error handling + integration test

## Files Likely Touched

- `src/influx.rs`
- `src/main.rs`
