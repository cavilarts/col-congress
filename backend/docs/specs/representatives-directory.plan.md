# Implementation Plan: Representatives Directory

Status: WIP (pending approval)
Functional spec: `backend/docs/specs/representatives-directory.functional.md`
Technical spec: `backend/docs/specs/representatives-directory.technical.md`
Mirrors: `backend/docs/specs/approved/senators-directory.plan.md`
Scope: backend only

## 0. Ground rules

- Same as the senators plan (section 0): each step ends with `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` passing.
- Unit tests are added in the same step as the logic.
- No commits unless asked.
- Dependency direction follows AGENTS.md: `domain` <- `application` <- `adapters`.

## 1. Preparation

1. Confirm the senators foundation is in place (senators plan, steps 1-10): `domain::normalize`, `datos_gov_co::client`, the PostgreSQL pool and migrations, the scheduler and the HTTP router. This plan adds to it and does not duplicate it.
2. Confirm the row count of `48i3-vuny` (`$select=count(*)`) and record it. Expected: 181 (technical spec, section 2.1).
3. Confirm the Congreso Visible listing page size: request `rows=500` and check that the page returns more than 24 cards. If the site caps the page size, the client pages with the cap (technical spec, section 2.2).
4. Create `backend/tests/fixtures/`.

## 2. Domain

Files: `src/domain/representative.rs`, `src/domain/mod.rs`.

1. Add `SourceKind::CongresoVisible` to the shared `SourceKind` enum (technical spec, section 4.1).
2. Add `Representative` with the fields in technical spec section 4.1, and `RepresentativeId`, derived from `(term, normalized_name)` as in technical spec section 4.3.
   - `constituency` is a new field. The senators' type does not have it.
3. Unit tests:
   - `RepresentativeId` slug for upper-case names with accents (`ABASOLO GÓMEZ ALEJANDRA GABRIELA` gives `2026-2030-abasolo-gomez-alejandra-gabriela`).
   - `normalize_name` on upper-case input.
   - `Term` validation is shared with the senators; add one case that is not already covered.

Verify: `domain` has no imports of `sqlx`, `reqwest`, `axum`, `tokio`, `scraper`.

## 3. Application layer

Files: `src/application/ports.rs` (extend), `src/application/sync_representatives.rs`, `src/application/list_representatives.rs`.

1. `ports.rs`: add `RepresentativeSource` and `RepresentativeRepository` (no `has_term`), as in technical spec 5.1.
2. `sync_representatives.rs`:
   - `refresh_current(primary, fallback: Option<_>)`: primary, then fallback, then `AllSourcesFailed`.
   - Count below 183: log `warn`, the run still succeeds.
   - `is_stale`: 24 hour rule.
   - Unit tests with in-memory fakes:
     - Primary succeeds: fallback never called.
     - Primary fails, fallback succeeds: upsert called with fallback data.
     - Both fail: no upsert, `AllSourcesFailed`.
     - Primary returns `Empty`: treated as failure.
     - No fallback and primary fails: `AllSourcesFailed`.
     - Count below 183: upsert still runs, warning logged.
     - Staleness: `None`, 23h, 25h.
3. `list_representatives.rs`: term validation and delegation. Tests as in the senators' step 4.

## 4. Outbound: datos.gov.co (`48i3-vuny`)

1. Fixture `tests/fixtures/datos_gov_co_representatives.json`: three real rows from `48i3-vuny`, one row with `ND` in the party, one row with a name in a different case, and one CITREP row.
2. `datos_gov_co::client`: take the dataset ID as a parameter. The senators keep `sjwx-dr6n`. Tests for the status mapping and the body-to-rows path, through a function that takes the body as a string.
3. `mapping.rs` for representatives:
   - `sesion` to `full_name`, `partido_politico` to `party`, `departamento` to `constituency`. Session columns ignored.
   - Unit tests: fixture produces the expected records; `ND` becomes `None`; blank name skipped and counted; duplicate names keep the first occurrence; session columns do not affect the output; a CITREP row maps correctly.

## 5. Outbound: Congreso Visible (fallback)

Files: `src/adapters/outbound/congreso_visible/{mod.rs, client.rs, parser.rs}`.

1. Fixture `tests/fixtures/congreso_visible_representatives.html`: one trimmed listing page with:
   - A multi-word surname.
   - A name with accents.
   - A party and a movement.
   - A card with `Comisión: Sin comisión`.
   - One duplicate name.
2. `parser.rs` (technical spec 6.2):
   - Full name built from surname text and given-name text.
   - Party kept as published, `constituency` `None`, `source` `CongresoVisible`.
   - Unit tests: card count, duplicate removed and counted, empty name skipped and counted, zero cards gives `Empty`, missing container gives `Parse`, commission text ignored.
3. `client.rs`: paged GET of the listing URL (`corporacion=1&cuatrienio=10`). Stops on a page with no profile links. Same timeout and `User-Agent`.
   - Unit tests: paging stops on an empty page; status mapping (`UnexpectedStatus`); a transport error gives `Network`.

## 6. Outbound: PostgreSQL

Files: `src/adapters/outbound/postgres/representative_repository.rs`, `backend/migrations/0002_create_representatives.sql`.

1. Create the migration as in technical spec section 7 (`source` allows `datos_gov_co` and `congreso_visible` only).
2. `representative_repository.rs`: `last_updated`, `list`, `upsert_all` (one transaction), `try_sync_lock` (separate advisory lock key from the senators).
3. Integration tests, gated on `TEST_DATABASE_URL`:
   - Migrations `0001` and `0002` apply to an empty database.
   - Upsert twice gives no duplicates.
   - `list` filters by term and orders by term desc then name.
   - Advisory lock: a second acquisition fails while the first guard is alive.
   - `source` rejects values other than `datos_gov_co` and `congreso_visible`.

## 7. HTTP inbound

Files: `src/adapters/inbound/http/representatives.rs`, `src/adapters/inbound/http/mod.rs` (extend).

1. Handler `GET /api/representatives`, query `term: Option<String>`. Errors and DTO as in technical spec section 8.
2. Add the route beside the senators' route. Keep `/health`.
3. Unit tests with `tower::ServiceExt::oneshot` and a fake repository:
   - 200 with JSON array and the expected keys, including `constituency`.
   - `?term=2026-2030` filters.
   - `?term=bad` gives 400.
   - Repository error gives 500 without leaking details.

## 8. Scheduler and composition root

1. `scheduler.rs`: add the representatives' task with its own window and lock. The loop code is shared with the senators (technical spec, section 9).
2. `main.rs`: wire the representatives' adapters and use cases. Run both migrations. Spawn one task per directory.
3. No test for `main.rs` (excluded from the coverage gate).

## 9. Coverage gate and documentation

1. Run `cargo llvm-cov --all-features --fail-under-lines 85 --ignore-filename-regex 'src/main.rs'` with `TEST_DATABASE_URL` set. Must pass.
2. Update `backend/README.md` or the root README only if the commands section needs the new endpoint or test commands.
3. Do not change AGENTS.md "Current state" unless asked.

## 10. Acceptance checklist

Maps to the functional spec's acceptance criteria.

- [ ] `GET /api/representatives` returns 2026-2030 records with the FR5 fields.
- [ ] `?term=2026-2030` returns only current-term representantes.
- [ ] With datos.gov.co unreachable, the endpoint serves the last stored data and the logs show the fallback attempt.
- [ ] A run below 183 stores what it parsed and logs the count, including the CITREP 2 gap.
- [ ] Running the sync twice creates no duplicates.
- [ ] Migrations create the schema on an empty PostgreSQL 17 database.
- [ ] `cargo test` passes.
- [ ] `cargo llvm-cov` reports at least 85% line coverage.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.

## 11. Risks

| Risk | Mitigation |
|------|------------|
| `48i3-vuny` has 181 rows against 183 seats (CITREP 2 missing) | Logged on every run. Reconcile against the CNE list once it can be reached (functional spec, section 7). |
| Column names in `48i3-vuny` change, or session columns grow each month | The mapping reads only `sesion`, `partido_politico` and `departamento`. Fixture tests catch renames. |
| Congreso Visible changes its HTML | Fixture tests catch parser breakage. The run keeps stored data. |
| Congreso Visible is a secondary source | Records are marked `congreso_visible`. The CNE list is the reconciliation reference. |
| Dataset stops being updated after the term's first months | Monitor `updatedAt` in the catalog. Stored data is kept if the source fails. |
| No historical data for 2018-2022 | Recorded as a limitation (functional spec, section 2). |
