# Implementation Plan: Senators Directory

Status: WIP (pending approval)
Functional spec: `backend/docs/specs/approved/senators-directory.functional.md`
Technical spec: `backend/docs/specs/approved/senators-directory.technical.md`
Scope: backend only

## 0. Ground rules

- Each step ends with `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` passing before moving on.
- Each step that adds logic adds its unit tests in the same step. No test-last work.
- Commits happen only when asked. The plan does not commit.
- Dependency direction follows AGENTS.md: `domain` <- `application` <- `adapters`. `main.rs` is the only place that wires concrete types.

## 1. Preparation

1. Verify `llvm-tools-preview` is installed: `rustup component add llvm-tools-preview`.
   - `cargo-llvm-cov` 0.8.5 is already installed.
2. Check the current baseline: `cargo test` and `cargo llvm-cov --summary-only` in `backend/`.
3. Create the folder `backend/tests/fixtures/`.
4. Check the Registraduría source (decision D1). Time box: one session. Record the result in the spec before continuing. Outcome options:
   - A structured source is found: add a task in step 9b.
   - None found: step 9b is skipped and the functional spec gets a note that the fallback is disabled in v1.

## 2. Dependencies

Add to `backend/Cargo.toml`:

- `sqlx` with features `runtime-tokio`, `postgres`, `migrate`, `chrono`.
- `reqwest` with `json`, `rustls-tls`, `default-features = false`.
- `serde` with `derive`, `serde_json`.
- `scraper`.
- `chrono`, `chrono-tz`.
- `rand`.
- `unicode-normalization`.
- `thiserror`.
- `tracing`, `tracing-subscriber` with `env-filter`.
- Dev only: `tower` (for `ServiceExt::oneshot` in handler tests), `http-body-util`.
- Extend `tokio` features with `time`.

Verify: `cargo check` passes.

## 3. Domain layer

Files: `src/domain/mod.rs`, `src/domain/senator.rs`, `src/domain/normalize.rs`.

1. `normalize.rs`: implement the functions in technical spec 4.2.
   - `clean_text`, `parse_gender`, `parse_votes`, `parse_birth_date_es`, `normalize_name`.
   - Unit tests: table-driven, covering every row of the spec table, plus `N/D`, empty, whitespace-only, and dates with uppercase months and missing `de` connectors.
2. `senator.rs`: types `Gender`, `SourceKind`, `Term` (validated `YYYY-YYYY`), `Senator`, and `SenatorId` with the derivation from technical spec 4.3.
   - Unit tests: `Term` rejects `2026`, `2026-30`, `abc`; accepts `2026-2030`. `SenatorId` slug for accented and punctuated names.

Verify: `domain` has no imports of `sqlx`, `reqwest`, `axum`, `tokio`, `scraper`. Check with `grep`.

## 4. Application layer

Files: `src/application/mod.rs`, `ports.rs`, `errors.rs`, `sync_senators.rs`, `list_senators.rs`.

1. `ports.rs`: `SenatorSource`, `SenatorRepository`, `Clock` traits as in technical spec 5.1, including `try_sync_lock` returning a guard type.
2. `errors.rs`: `SourceError`, `RepositoryError`, `SyncError`, `ListError` (`InvalidTerm`, `Repository`). Follow technical spec section 14 (error types, flow behavior, log levels). Add `ConfigError` and `StartupError` in `main.rs` or a small `config.rs`.
3. `sync_senators.rs`:
   - `load_historical`: skip when the term exists; otherwise fetch and upsert.
   - `refresh_current`: primary, then fallback, then `AllSourcesFailed`. Logs each failure.
   - `is_stale`: 24 hour rule.
   - Unit tests with in-memory fakes:
     - Primary succeeds: fallback never called.
     - Primary fails, fallback succeeds: upsert called with fallback data.
     - Both fail: no upsert, `AllSourcesFailed`.
     - Primary returns `Empty`: treated as failure.
     - Historical present: skipped, source never called.
     - Staleness: `None`, 23h, 25h.
4. `list_senators.rs`: term validation and delegation.
   - Unit tests: valid term passes through, invalid term is `InvalidTerm`, `None` lists everything.

Verify: `application` imports only `domain` and `std` plus `chrono` and `thiserror`.

## 5. Scheduler logic (inbound, pure part)

File: `src/adapters/inbound/scheduler.rs`.

1. Implement `next_run_after(now: DateTime<Utc>, offset_seconds: u32) -> DateTime<Utc>`:
   - Window 02:00 to 05:00 `America/Bogota`.
   - Returns the next occurrence strictly after `now`.
   - The offset is in `0..10800` seconds, passed in by the caller.
2. Unit tests with fixed inputs:
   - `now` at 01:00 Bogotá: today 02:00 + offset.
   - `now` at 04:59:59 Bogotá: next day.
   - `now` at 05:00 exactly: next day.
   - Crossing midnight and DST-free behavior (Colombia has no DST; assert it).

The async loop (startup check, sleep, lock, run) is written in step 8 with the wiring.

## 6. Outbound: datos.gov.co

Files: `src/adapters/outbound/datos_gov_co/{mod.rs, client.rs, mapping.rs}`.

1. Fixture `tests/fixtures/datos_gov_co_senators.json`: 3 real rows from the SODA response, plus one row with `ND` values and one with an invalid birth date.
2. `mapping.rs`: raw serde struct -> `Senator` using the normalizers. Rows without a name are skipped and counted.
   - Unit tests: fixture produces expected `Senator`s; `ND` becomes `None`; invalid date becomes `None`; blank name skipped.
3. `client.rs`: `SenatorSource` impl.
   - URL: `{base}/resource/sjwx-dr6n.json?$limit=1000&$where=periodo='2018-2022'`.
   - Optional `X-App-Token` from `DATOS_GOV_CO_APP_TOKEN`. Never logged.
   - Timeout 30 seconds. `User-Agent` `congreso-backend/<version>`.
   - Maps non-2xx to `UnexpectedStatus`, transport errors to `Network`, empty array to `Empty`.
   - Unit tests: status mapping and the body-to-senators path through a function that takes the body as a string, so no network is needed.

## 7. Outbound: Senado website

Files: `src/adapters/outbound/senado_web/{mod.rs, client.rs, parser.rs}`.

1. Fixture `tests/fixtures/senado_senators.html`: trimmed copy of the real page, containing:
   - Cards from both `misc_off` and `misc_on` blocks.
   - One card with empty party.
   - One phone with `EXT 5051 - 5052`.
   - One card with a website `p`.
   - One duplicated name across the two blocks.
   - Cut the page down to the directory section so the fixture stays small.
2. `parser.rs`: implement technical spec 6.2.
   - Unit tests: card count, dedupe count, phone and extension split, empty party is `None`, website detected, name trimmed, empty name skipped, no cards gives `Empty`.
3. `client.rs`: `GET .../senadores?lastletter=Todos`. Same timeout and user agent. Status mapping as in step 6.

## 8. Outbound: PostgreSQL

Files: `src/adapters/outbound/postgres/{mod.rs, senator_repository.rs}`, `backend/migrations/0001_create_senators.sql`.

1. Create the migration exactly as in technical spec section 7.
2. `senator_repository.rs`:
   - `has_term`, `last_updated`, `list`, `upsert_all` (one transaction, `ON CONFLICT (id) DO UPDATE`), `try_sync_lock` (`pg_try_advisory_lock` on a fixed key, guard releases on drop).
   - Runtime queries only, no `query!` macros.
3. Integration tests, gated on `TEST_DATABASE_URL`. If unset, the test prints a skip message and returns:
   - Migrations apply to an empty database.
   - Upsert twice gives no duplicates.
   - Upsert updates `updated_at`.
   - `list` filters by term and orders by term desc then name.
   - `last_updated` returns `None` for an empty term.
   - Advisory lock: second acquisition fails while the first guard is alive.
4. Start a local PostgreSQL 17 for the tests: `podman compose up -d postgres`, then `TEST_DATABASE_URL=postgres://congreso:congreso@127.0.0.1:5433/congreso`.
   - Check the credentials in `compose.yaml` first; use the values there and do not hardcode new ones.

## 9. HTTP inbound

Files: `src/adapters/inbound/http/{mod.rs, senators.rs}`.

1. `senators.rs`: handler `GET /api/senators`, query `term: Option<String>`.
   - `ListError::InvalidTerm` -> 400 with `{"error":"invalid_term"}`.
   - Repository error -> 500 with `{"error":"internal"}`, details logged.
   - Response DTO per technical spec 8.2, with a `From<Senator>` conversion.
2. `mod.rs`: keep `/health`. Add `/api/senators`. The router takes the list use case as state.
3. Unit tests with `tower::ServiceExt::oneshot` and a fake repository:
   - 200 with JSON array and the expected keys.
   - `?term=2026-2030` filters.
   - `?term=bad` gives 400.
   - Repository error gives 500 without leaking details.
   - `/health` still returns `ok`.

## 10. Scheduler (async loop) and composition root

1. `scheduler.rs`: async loop from technical spec 9:
   - Startup: load historical if absent; refresh current if stale.
   - Daily loop: `next_run_after` with `rand` offset, sleep, try lock, run `refresh_current`, log outcome.
   - Pass in the use cases and `Clock`. Keep the loop thin.
   - Unit test the loop's decision step with a fake clock, if practical. Otherwise cover through the pure function from step 5.
2. `main.rs`:
   - Initialize `tracing-subscriber` with `RUST_LOG`.
   - Read `DATABASE_URL` (required, fail with a clear message), `BIND_ADDRESS` (default `0.0.0.0:3000`), `DATOS_GOV_CO_APP_TOKEN` (optional).
   - Create the `PgPool`, run `sqlx::migrate!("./migrations")`.
   - Build the adapters and use cases, spawn the scheduler, serve the router.
   - No test for `main.rs`. It is excluded from the coverage gate.

## 11. Registraduría fallback (conditional)

Only if step 1.4 found a usable source.

1. Add `adapters/outbound/registraduria/` with client, parser and mapping, following the same pattern as step 7.
2. Map its fields to `Senator` with `source = Registraduria`. Fields it does not provide are `None`.
3. Unit tests with fixtures, same coverage standard.
4. Wire it as the fallback in `main.rs`.

If not found: skip this section. Wire `refresh_current` with the primary source only, and pass a fallback that always returns `Err(SourceError::Network)`... Prefer an `Option<S>` fallback in the use case so no dummy source is needed. Decide in step 1.4 and record it in the technical spec.

## 12. Container and compose

1. `backend/Containerfile`: add `COPY migrations ./migrations` in the build stage, next to `COPY src ./src`.
2. `compose.yaml`: add `DATABASE_URL` to the backend service, built from the existing `POSTGRES_*` variables and pointing at host `postgres`, port 5432 inside the network.
3. Build and run: `podman compose up --build -d`, then `podman compose logs backend`.
   - Expect "migrations applied" and the startup sync log lines.
4. Smoke test: `curl http://localhost:3000/api/senators?term=2026-2030` and `curl "http://localhost:3000/api/senators?term=2018-2022"`.
   - Requires network access to the external sources. If the Senado site is unreachable from the container, that is an expected finding, not a failure of this plan. Record it.

## 13. Coverage gate and documentation

1. Run `cargo llvm-cov --all-features --fail-under-lines 85 --ignore-filename-regex 'src/main.rs'` with `TEST_DATABASE_URL` set. Must pass.
   - If below 85%, add unit tests for the uncovered branches. Do not lower the threshold.
2. Update `backend/README.md` or the root README only if the commands section needs the new test and coverage commands. Keep changes minimal and in line with AGENTS.md.
3. Update AGENTS.md "Current state" only if asked. Do not change it on our own.

## 14. Acceptance checklist

Maps to functional spec acceptance criteria.

- [ ] `GET /api/senators` returns both terms with the FR5 fields.
- [ ] `?term=2026-2030` returns only current-term senators.
- [ ] With the Senado site unreachable, the endpoint serves the last stored data and the logs show the fallback attempt.
- [ ] An unparseable birth date is stored as `null` and logged.
- [ ] Running the sync twice creates no duplicates.
- [ ] Migrations create the schema on an empty PostgreSQL 17 database.
- [ ] `cargo test` passes.
- [ ] `cargo llvm-cov` reports at least 85% line coverage.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] The composed backend starts, applies migrations, and serves `/api/senators`.

## 15. Risks

| Risk | Mitigation |
|------|------------|
| Senado HTML changes | Fixture tests catch parser breakage. The sync logs parse failures and keeps stored data. |
| Registraduría has no usable source | Time-boxed investigation in step 1.4, fallback becomes optional. |
| Only 102 unique names parsed for 2026-2030 | Log the count on every run. Compare against the official count before declaring the sync complete. |
| datos.gov.co rate limits | One request per run; optional app token. |
| Coverage below 85% because of the DB adapter | Integration tests run against the compose PostgreSQL; `TEST_DATABASE_URL` required for the gate. |
| Container cannot reach external sources | Documented as an environment finding in step 12.4. |
