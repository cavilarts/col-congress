# Technical Spec: Senators Directory

Status: WIP (pending approval)
Functional spec: `backend/docs/specs/approved/senators-directory.functional.md`
Scope: backend only

## 1. Overview

The backend loads senators from three sources into PostgreSQL and serves them through `GET /api/senators`.

| Term      | Source                                   | Access method                               | When                       |
|-----------|------------------------------------------|---------------------------------------------|----------------------------|
| 2018-2022 | datos.gov.co `sjwx-dr6n`                 | HTTP GET, Socrata SODA JSON                 | Once, if the term is absent |
| 2026-2030 | Senado website, senators page            | HTTP GET, HTML parsing                      | Daily, 02:00-05:00 Bogotá   |
| 2026-2030 | Registraduría (fallback)                 | To be defined (see section 11)              | Only if the primary fails   |

The API never calls the external sources. Requests read only from PostgreSQL.

## 2. Verified source facts

Verified during planning, not assumed.

### 2.1 datos.gov.co (Socrata)

- Endpoint: `https://www.datos.gov.co/resource/sjwx-dr6n.json`. Returns a JSON array. No token required.
- Default page size is 1000 rows. The client always sends `$limit` explicitly.
- Field names used: `periodo`, `nombre`, `g_nero`, `a_o_de_nacimiento`, `departamento_de_nacimiento`, `ciudad_de_nacimiento`, `partido`, `n_mero_de_votos`, `comisi_n_constitucional`, `comisi_n_legal`, `correo_electr_nico`, `twitter`, `facebook`, `pagina_web`, `telefono`, `ext`.
- `codigo_dane` is not used. It holds a department code, and the functional spec does not include it.
- Values are all text, including `ext` and `n_mero_de_votos`. Placeholder `"ND"` is common.

### 2.2 Senado website

- URL: `https://www.senado.gov.co/index.php/el-senado/senadores?lastletter=Todos`
- Server-rendered Joomla HTML. No JavaScript is needed to read the list.
- `lastletter=Todos` returns the whole list in one response. This satisfies the "one request per run" rule in the functional spec.
- Each senator is a `div.modazdirectory__result` card with:
  - `h3`: full name, in the form `Apellido Apellido Nombre Nombre`. May have trailing spaces.
  - `p` right after `h3`: party. May be empty.
  - `p` containing `span.modazdirectory__label-phone`: phone text after the label, e.g. `(57)(601) 3823000 EXT 3610`.
  - `p.modazdirectory__nowrap`: email, protected by JavaScript obfuscation. Not used, see FR decision D3.
  - Some cards have a plain `p` starting with `http` for the website.
- The page renders each senator twice: once in a `modazdirectory__layout-misc_off` block and again in a `modazdirectory__layout-misc_on` block. Snapshot: 204 cards, 102 unique names.
- Not present on the list page: gender, birth date, birth place, votes, commissions, Twitter, Facebook. These fields are `null` for the current term (see section 11, item 2).

### 2.3 Registraduría

Not verified. The result pages found during planning are for popular consultations and election results, not a directory. Section 11, item 1.

## 3. Architecture

Hexagonal layout from AGENTS.md. Dependencies point inward.

```
backend/
├── migrations/
│   └── 0001_create_senators.sql
├── tests/
│   └── fixtures/
│       ├── datos_gov_co_senators.json
│       └── senado_senators.html
└── src/
    ├── main.rs                         composition root: config, pool, migrate, wiring, serve
    ├── domain/
    │   ├── mod.rs
    │   ├── senator.rs                  Senator, Gender, Term, Source, SenatorId
    │   └── normalize.rs                pure parsing and cleaning rules
    ├── application/
    │   ├── mod.rs
    │   ├── ports.rs                    SenatorSource, SenatorRepository, Clock
    │   ├── errors.rs                   SourceError, RepositoryError, SyncError
    │   ├── sync_senators.rs            SyncSenators use case
    │   └── list_senators.rs            ListSenators use case
    └── adapters/
        ├── inbound/
        │   ├── mod.rs
        │   ├── http/
        │   │   ├── mod.rs              router (keeps /health)
        │   │   └── senators.rs         GET /api/senators handler + response DTO
        │   └── scheduler.rs            daily 02:00-05:00 trigger and startup check
        └── outbound/
            ├── mod.rs
            ├── datos_gov_co/
            │   ├── mod.rs
            │   ├── client.rs           SenatorSource impl, HTTP call
            │   └── mapping.rs          raw JSON row -> Senator
            ├── senado_web/
            │   ├── mod.rs
            │   ├── client.rs           SenatorSource impl, HTTP call
            │   └── parser.rs           HTML -> Vec<Senator>
            ├── registraduria/          see section 11, item 1
            └── postgres/
                ├── mod.rs
                └── senator_repository.rs   SenatorRepository impl (sqlx)
```

Notes:

- `domain` has no `sqlx`, `reqwest`, `axum`, `tokio` or `scraper` imports.
- `application` depends only on `domain` and on its own ports. It uses no framework types.
- `adapters/outbound/*` implement the ports. The HTTP and HTML parsing stay in the adapters.
- `main.rs` is the only place that knows the concrete types.

## 4. Domain model

### 4.1 Types

```rust
pub enum Gender { Male, Female, Unknown }

pub enum SourceKind { DatosGovCo, Senado, Registraduria }

pub struct Term(String);            // "2018-2022", validated as YYYY-YYYY

pub struct Senator {
    pub id: SenatorId,              // derived, see 4.3
    pub term: Term,
    pub full_name: String,
    pub normalized_name: String,
    pub gender: Gender,
    pub birth_date: Option<chrono::NaiveDate>,
    pub department_of_birth: Option<String>,
    pub city_of_birth: Option<String>,
    pub party: Option<String>,
    pub votes: Option<u64>,
    pub constitutional_commission: Option<String>,
    pub legal_commission: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub phone_extension: Option<String>,
    pub website: Option<String>,
    pub twitter: Option<String>,
    pub facebook: Option<String>,
    pub source: SourceKind,
}
```

`updated_at` is set by the repository, not by the domain. The clock is passed in through the `Clock` port.

### 4.2 Normalization rules (`domain::normalize`)

All pure functions, all unit tested.

| Function                   | Input example                  | Output                          |
|----------------------------|--------------------------------|---------------------------------|
| `clean_text(&str)`         | `"  Polo  Democrático "`       | `Some("Polo Democrático")`      |
| `clean_text(&str)`         | `"ND"`, `""`, `"   "`          | `None`                          |
| `parse_gender(&str)`       | `"masculino"`, `"Femenino"`    | `Male`, `Female`                |
| `parse_gender(&str)`       | `"ND"`, `"otro"`               | `Unknown`                       |
| `parse_votes(&str)`        | `"82,854"`                     | `Some(82854)`                   |
| `parse_votes(&str)`        | `"ND"`, `"abc"`                | `None`                          |
| `parse_birth_date_es(&str)`| `"25 de marzo de 1971"`        | `Some(1971-03-25)`              |
| `parse_birth_date_es(&str)`| `"22 de noviembre de 1066"`    | `None` (invalid year range)     |
| `parse_birth_date_es(&str)`| `"18 de Julio e 1950"`         | `None` (unparseable)            |
| `normalize_name(&str)`     | `"Acuña Díaz Laureano Augusto"`| `"acuna diaz laureano augusto"` |

Rules:

- `clean_text` trims and collapses internal whitespace. Case is kept.
- Placeholders `ND` and `N/D` (case-insensitive) become `None`.
- `parse_birth_date_es` accepts a day, a Spanish month name in any case, and a 4-digit year. The `de` connectors are optional. The year must be between 1900 and the current year minus 18 to be accepted. Values outside this range or unparseable become `None`, and the caller logs the record id.
- `normalize_name` lowercases, removes diacritics via Unicode NFD and dropping combining marks, and collapses whitespace.
- `parse_votes` strips `,` and `.` thousands separators. Values that are not integers become `None`.

### 4.3 Identity

- `normalized_name` = `normalize_name(full_name)`.
- `id` = `"{term}-{slug}"`, where `slug` replaces each run of non-alphanumeric characters with `-`, trims `-`, and is applied to `normalized_name`.
  - Example: `2026-2030-ana-paola-garcia` (illustrative format).
- The pair `(term, normalized_name)` is unique in the database. `id` is derived from it, so the two stay in sync.

### 4.4 Decision D3: current-term emails

The Senado list page does not expose emails in plain text. The functional spec requires `email = null` for the current term. The parser does not decode the obfuscated value.

## 5. Application layer

### 5.1 Ports (`application/ports.rs`)

```rust
pub trait SenatorSource {
    fn kind(&self) -> SourceKind;
    fn fetch(&self, term: &Term) -> impl Future<Output = Result<Vec<Senator>, SourceError>> + Send;
}

pub trait SenatorRepository {
    fn has_term(&self, term: &Term) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    fn last_updated(&self, term: &Term) -> impl Future<Output = Result<Option<DateTime<Utc>>, RepositoryError>> + Send;
    fn upsert_all(&self, senators: &[Senator], now: DateTime<Utc>) -> impl Future<Output = Result<usize, RepositoryError>> + Send;
    fn list(&self, term: Option<&Term>) -> impl Future<Output = Result<Vec<Senator>, RepositoryError>> + Send;
    fn try_sync_lock(&self) -> impl Future<Output = Result<Option<SyncLock>, RepositoryError>> + Send;
}

pub trait Clock {
    fn now(&self) -> DateTime<Utc>;
}
```

Native `async fn` in traits, generic use cases. No `async-trait` crate.

Errors (`application/errors.rs`):

- `SourceError`: `Network`, `UnexpectedStatus(u16)`, `Parse(String)`, `Empty`.
- `RepositoryError`: `Storage(String)`.
- `SyncError`: `AllSourcesFailed`, `Repository(RepositoryError)`.

### 5.2 `SyncSenators` use case

Two operations.

**`load_historical(dataset_source)`**

1. If `repo.has_term("2018-2022")` is true, return `Skipped`.
2. Fetch from the datos.gov.co source for `2018-2022`.
3. `repo.upsert_all`, return `Loaded(count)`.

**`refresh_current(primary, fallback)`**

1. Try `primary.fetch("2026-2030")`.
2. If it fails (any `SourceError`, including `Empty`), log the error and try `fallback.fetch("2026-2030")`.
3. If the fallback also fails, return `SyncError::AllSourcesFailed`. The stored data is not touched.
4. On success, `repo.upsert_all`, which runs in a single transaction (see 6.3).

**`is_stale(now, last_updated)`**

- `true` if `last_updated` is `None` or older than 24 hours.

Both operations take an `&impl Clock`. `now` comes from the clock, which makes the logic testable.

### 5.3 `ListSenators` use case

- `execute(term: Option<Term>) -> Vec<Senator>`. Delegates to `repo.list`.
- Term validation (format `YYYY-YYYY`) happens here, not in the handler. An invalid format returns `ListError::InvalidTerm`, mapped to HTTP 400.
- Unknown but well-formed terms return an empty list (functional spec FR4).

## 6. Outbound adapters

### 6.1 datos.gov.co (`adapters/outbound/datos_gov_co`)

- `reqwest::Client` with a 30 second timeout and a `User-Agent` of `congreso-backend/<version>`.
- Request: `GET {base}/resource/sjwx-dr6n.json?$limit=1000&$where=periodo='2018-2022'`. The `$where` clause keeps the request small and avoids unrelated rows if the dataset changes.
- Optional token from `DATOS_GOV_CO_APP_TOKEN`, sent in the `X-App-Token` header when set. Never logged.
- `mapping.rs` converts each row:
  - `id`, `term`, `normalized_name`, `full_name` from `nombre`.
  - `gender` from `g_nero` via `parse_gender`.
  - `birth_date` from `a_o_de_nacimiento` via `parse_birth_date_es`.
  - `votes` from `n_mero_de_votos` via `parse_votes`.
  - Other text fields via `clean_text`.
  - `email` from `correo_electr_nico`.
  - `phone_extension` from `ext`, `clean_text`.
- Rows without a usable name are skipped and counted in the log.

### 6.2 Senado website (`adapters/outbound/senado_web`)

Client:

- `GET https://www.senado.gov.co/index.php/el-senado/senadores?lastletter=Todos`
- Same timeout and `User-Agent` as the datos.gov.co client.
- Non-2xx status becomes `SourceError::UnexpectedStatus`.

Parser (`parser.rs`), using the `scraper` crate:

1. Select `div.modazdirectory__result`.
2. For each card:
   - `name` = trimmed text of `h3`. Empty name: skip the card.
   - `party` = `clean_text` of the first `p` after `h3`.
   - Phone: find the `p` containing `span.modazdirectory__label-phone`. Remove the label text. Split on `EXT` (case-insensitive):
     - Left part: `phone` (cleaned).
     - Right part: `phone_extension` (cleaned). Example `EXT 5051 - 5052` gives `5051 - 5052`.
   - `website` = text of the first `p` whose text starts with `http` or `www`, if present.
   - `email`, `gender`, `birth_date`, `department_of_birth`, `city_of_birth`, `votes`, both commissions, `twitter`, `facebook`: `None` or `Unknown`, see 2.2 and section 11, item 2.
3. Deduplicate by `normalized_name`, keeping the first occurrence. Log the number of duplicates removed.
4. If the result is empty, return `SourceError::Empty`.

Parser tests use `tests/fixtures/senado_senators.html`, a trimmed copy of the real page with at least one card from each layout block, one empty party, one `EXT` phone, one card with a website, and one duplicate name.

### 6.3 PostgreSQL (`adapters/outbound/postgres`)

- `sqlx::PgPool`, created in `main.rs` from `DATABASE_URL`. Pool size default.
- Runtime queries (`sqlx::query` / `query_as`), not the compile-time `query!` macros. This keeps builds from needing a live database.
- Migrations embedded with `sqlx::migrate!("./migrations")` and run at startup before serving.
- `upsert_all`:
  - Opens one transaction.
  - For each senator: `INSERT ... ON CONFLICT (id) DO UPDATE SET <all columns>, updated_at = EXCLUDED.updated_at`.
  - Commits. Any error rolls back the whole batch (FR3).
- `try_sync_lock`: `pg_try_advisory_lock` on a fixed key. Returns a guard that releases the lock on drop. Prevents two instances from syncing at once (section 11, item 5).
- `list`: `SELECT ... FROM senators [WHERE term = $1] ORDER BY term DESC, full_name`.
- `last_updated`: `SELECT MAX(updated_at) FROM senators WHERE term = $1`.

## 7. Migrations

`backend/migrations/0001_create_senators.sql`:

```sql
CREATE TABLE senators (
    id                          TEXT PRIMARY KEY,
    term                        TEXT NOT NULL,
    normalized_name             TEXT NOT NULL,
    full_name                   TEXT NOT NULL,
    gender                      TEXT NOT NULL CHECK (gender IN ('male', 'female', 'unknown')),
    birth_date                  DATE,
    department_of_birth         TEXT,
    city_of_birth               TEXT,
    party                       TEXT,
    votes                       BIGINT CHECK (votes IS NULL OR votes >= 0),
    constitutional_commission   TEXT,
    legal_commission            TEXT,
    email                       TEXT,
    phone                       TEXT,
    phone_extension             TEXT,
    website                     TEXT,
    twitter                     TEXT,
    facebook                    TEXT,
    source                      TEXT NOT NULL CHECK (source IN ('datos_gov_co', 'senado', 'registraduria')),
    updated_at                  TIMESTAMPTZ NOT NULL,
    UNIQUE (term, normalized_name)
);

CREATE INDEX senators_term_idx ON senators (term);
```

Only one migration. No seed data.

## 8. HTTP (inbound)

### 8.1 Endpoint

`GET /api/senators[?term=YYYY-YYYY]`

- `200 OK` with a JSON array.
- `400 Bad Request` if `term` is present and not `YYYY-YYYY`. Body: `{"error": "invalid_term"}`.
- `500 Internal Server Error` on database failure. Body: `{"error": "internal"}`. The details go to the log, not the response.
- `/health` stays as it is.

### 8.2 Response DTO

```json
{
  "id": "2026-2030-...",
  "term": "2026-2030",
  "full_name": "Agudelo García Ana Paola",
  "gender": "unknown",
  "birth_date": null,
  "department_of_birth": null,
  "city_of_birth": null,
  "party": "MIRA",
  "votes": null,
  "constitutional_commission": null,
  "legal_commission": null,
  "email": null,
  "phone": "(57)(601) 3823000",
  "phone_extension": "3610",
  "website": null,
  "twitter": null,
  "facebook": null,
  "source": "senado",
  "updated_at": "2026-10-11T07:00:00Z"
}
```

- `birth_date` is `YYYY-MM-DD` or `null`.
- `updated_at` is RFC 3339 UTC.
- The DTO is a separate type in `adapters/inbound/http/senators.rs` with `serde::Serialize`. The domain type is not serialized directly.

## 9. Scheduler (inbound)

`adapters/inbound/scheduler.rs`.

**Startup**

1. Load the historical dataset if `has_term("2018-2022")` is false.
2. If `is_stale(now, last_updated("2026-2030"))`, run `refresh_current` once.
3. Start the daily loop.

**Daily loop**

1. Compute the next run as a random instant between 02:00 and 05:00 in `America/Bogota`, on the next day that has not run yet.
2. Sleep until that instant with `tokio::time::sleep`.
3. Acquire `try_sync_lock`. If it is not acquired, log and skip.
4. Run `refresh_current`. Log the outcome (source used, count, duration).
5. Go back to step 1.

**Timezone**

The functional spec says "server local time". This spec uses `America/Bogota` through the `chrono-tz` crate, so the window is Colombian time regardless of the container's timezone. See section 11, item 3.

**Testability**

The next-run calculation is a pure function `next_run_after(now: DateTime<Utc>, rng_seconds: u32) -> DateTime<Utc>`. The random value is passed in, so tests are deterministic.

## 10. Configuration and dependencies

### 10.1 Environment variables

| Variable                   | Required | Default                | Used by                     |
|----------------------------|----------|------------------------|-----------------------------|
| `DATABASE_URL`             | yes      | none                   | `main.rs` (pool)            |
| `BIND_ADDRESS`             | no       | `0.0.0.0:3000`         | `main.rs`                   |
| `DATOS_GOV_CO_APP_TOKEN`   | no       | none                   | datos.gov.co client         |
| `RUST_LOG`                 | no       | `info`                 | `tracing-subscriber`        |

Credentials are never in code. `compose.yaml` passes `DATABASE_URL` to the backend service.

### 10.2 New crates

| Crate                 | Purpose                                              |
|-----------------------|------------------------------------------------------|
| `sqlx`                | PostgreSQL access and migrations (`runtime-tokio`, `postgres`, `migrate`, `chrono`) |
| `reqwest`             | HTTP client (`json`, `rustls-tls`, `default-features = false`) |
| `serde`, `serde_json` | Deserialize the datos.gov.co JSON; serialize the API |
| `scraper`             | HTML parsing for the Senado page                     |
| `chrono`              | Dates and timestamps                                 |
| `chrono-tz`           | `America/Bogota` for the scheduler window            |
| `rand`                | Random time inside the window                        |
| `unicode-normalization` | Remove diacritics for `normalize_name`             |
| `thiserror`           | Error types                                          |
| `tracing`, `tracing-subscriber` | Structured logs                            |

Existing: `axum` 0.8, `tokio` 1 (`macros`, `rt-multi-thread`). Add `time` to tokio features for `tokio::time::sleep`.

Versions are picked at implementation time as the latest compatible releases. The lockfile is committed as usual.

### 10.3 Compose and Containerfile

- `compose.yaml`: add `DATABASE_URL` to the backend service, pointing at the `postgres` service, using the existing `POSTGRES_*` values. Keep the `depends_on` as it is.
- `backend/Containerfile`: must copy `migrations/` into the image, because `sqlx::migrate!` reads them at compile time. To be verified during implementation.

## 11. Decisions

Status: WIP revision. Adds section 14 (error handling). Requires re-approval.

1. **Registraduría fallback. Decided: option (a). Result (2026-10-11): no structured source found; the fallback is disabled in v1.** Checked: the Registraduría election results on datos.gov.co (2018 Senate results, `75f2-fe2s`) and the Registraduría CSV archive on cedae.datasketch.co (stops at 2019) contain election results, not a directory of current senators. The 2026 Congress escrutinio page is script-driven and has no list in its HTML. The use case takes an optional fallback (`Option<S>`), so the run uses the primary source only. The fallback is not a dummy source. The original reasoning follows. The pages found during planning are for popular consultations and results, not a directory of senators. Options:
   - (a) Investigate a structured Registraduría source during implementation. If none exists, the fallback is disabled in v1 and the functional spec is updated to say so.
   - (b) Mark the fallback as deferred now and update the functional spec.
   - Recommendation: (a) with a time box.

2. **Missing fields for the current term. Decided: accept `null`.** The Senado list page does not include gender, birth date, birth place, votes, commissions, Twitter or Facebook. For 2026-2030 those fields will be `null`. The functional spec does not say this. Options:
   - (a) Accept nulls for these fields in v1 and document it in the functional spec.
   - (b) Add a second source for those fields. This is a larger scope change.
   - Recommendation: (a).

3. **Timezone. Decided: `America/Bogota`.** The functional spec says "server local time". This spec proposes `America/Bogota`. Confirm.

4. **Coverage scope. Decided: confirmed as written.**
   - The 85% line-coverage gate uses `cargo llvm-cov`. The coverage run includes the PostgreSQL repository integration tests, which need a database. These tests run only when `TEST_DATABASE_URL` is set, and CI or the developer must provide one.
   - `main.rs` (composition root) is excluded from the gate. Everything else counts.
   - Confirm both points.

5. **Multiple instances. Decided: keep the advisory lock.** The advisory lock (6.3) guards against two backend instances syncing at the same time. It is cheap, so this spec includes it. Confirm, or drop it if you only plan one instance.

## 12. Testing strategy

Unit tests are mandatory. The gate is at least 85% line coverage, measured with:

```sh
cargo llvm-cov --all-features --fail-under-lines 85 --ignore-filename-regex 'src/main.rs'
```

Requires the `llvm-tools-preview` rustup component (to check during implementation).

| Area                          | Test type                 | Coverage target |
|-------------------------------|---------------------------|-----------------|
| `domain::normalize`           | Unit, table-driven        | 100% of branches |
| `domain::senator` (id, types) | Unit                      | Full            |
| `application::sync_senators`  | Unit, fake source/repo/clock | Full (fallback, empty, skip, stale) |
| `application::list_senators`  | Unit, fake repo           | Full            |
| `scheduler::next_run_after`   | Unit, fixed inputs        | Full            |
| `senado_web::parser`          | Unit, HTML fixture        | Full            |
| `datos_gov_co::mapping`       | Unit, JSON fixture        | Full            |
| `http::senators` handler      | Unit, `tower::ServiceExt::oneshot` with fake repo | Full |
| `postgres::senator_repository`| Integration, `TEST_DATABASE_URL` | Full when DB available |
| Clients (`reqwest` calls)     | Thin, error mapping unit tested via pure functions | Best effort |

No test performs a live network call. Fixtures live in `backend/tests/fixtures/`.

Fake implementations of the ports live in test modules, not in production code.

## 13. Acceptance mapping

| Functional acceptance criterion                                 | Technical coverage                       |
|-----------------------------------------------------------------|------------------------------------------|
| 1. Endpoint returns both terms with FR5 fields                  | 8.2 DTO, sync use cases, integration test |
| 2. `?term=2026-2030` returns only current term                  | 5.3, repository `list`                   |
| 3. Senado down, endpoint serves stored data                     | 5.2 (stored data not touched), 8.1        |
| 4. Unparseable birth date stored as null and logged             | 4.2, 6.1                                  |
| 5. Running the daily job twice creates no duplicates            | 6.3 upsert on `id`                        |
| 6. Migrations create schema on empty PostgreSQL 17              | 7, migration runs at startup              |
| 7. `cargo test` passes, `cargo llvm-cov` at least 85%           | 12                                        |

## 14. Error handling

### 14.1 Principles

- Errors are values. The request path and the scheduler never panic. `unwrap` and `expect` are allowed only in `main.rs` during startup configuration.
- Each layer owns its error types. Adapters convert library errors (`reqwest`, `sqlx`, `scraper`, `serde_json`) into port errors. Library types never cross a port.
- Data quality problems are not errors. Field-level problems become `None` (domain normalizers return `Option`). Record-level problems skip the record. Only source-level, storage-level and configuration failures are errors.
- Application errors carry no HTTP meaning. The mapping to status codes happens only in `adapters/inbound/http`.
- Logs never contain the database URL, the app token, or raw response bodies. Parse errors log the reason and the position, not the content.

### 14.2 Error types and their meaning

| Type | Variant | Meaning | Raised by | Outcome |
|------|---------|---------|-----------|---------|
| `SourceError` | `Network` | Timeout, DNS or connection failure | datos.gov.co and Senado clients | Source failed. Counts as a failure for the flow (see 14.3). |
| `SourceError` | `UnexpectedStatus(u16)` | Non-2xx HTTP response | Same | Source failed. |
| `SourceError` | `Parse(String)` | Structure not as expected: JSON not an array, or Senado container selector missing from the page | Same | Source failed. The string is a short reason, not the body. |
| `SourceError` | `Empty` | Source responded but yielded zero valid senators | Same | Source failed. Stored data is kept. |
| `RepositoryError` | `Storage(String)` | Query, connection or transaction failure | PostgreSQL adapter | Run failed. Transaction rolled back. |
| `SyncError` | `AllSourcesFailed` | Primary and fallback both failed | `refresh_current` | Run failed. Stored data unchanged. |
| `SyncError` | `Repository(RepositoryError)` | Storage failed during a run | Use cases | Run failed. |
| `ListError` | `InvalidTerm` | `term` query parameter not `YYYY-YYYY` | `list_senators` | HTTP 400. |
| `ListError` | `Repository(RepositoryError)` | Storage failed during a read | `list_senators` | HTTP 500. |
| `ConfigError` | `Missing(&'static str)`, `Invalid(&'static str)` | Required variable absent or malformed | `main.rs` | Process exits non-zero before serving. |
| `StartupError` | `Migration`, `Connect` | Pool or migration failed at startup | `main.rs` | Process exits non-zero before serving. |

Conversions:

- `reqwest::Error` -> `SourceError::Network` (timeouts and connection) or `UnexpectedStatus` (via `status()`).
- `sqlx::Error` -> `RepositoryError::Storage`, with the message stripped of connection details.
- `serde_json::Error` and scraper selector misses -> `SourceError::Parse`.

### 14.3 Behavior per flow

**Historical load (`load_historical`)**

- A failure logs at `error`, does not mark the term as loaded, and returns. Nothing is written.
- The next daily run and the next startup retry it, because `has_term` is still false.

**Current refresh (`refresh_current`)**

1. Primary fails: log `warn` with source kind and error kind, then try the fallback.
2. Fallback fails or is absent: log `error`, return `AllSourcesFailed`. Stored data is not touched.
3. Success with zero senators is `Empty`, which counts as a failure in step 1.
4. No retry within a single run. One attempt per source per run, as the functional spec requires.

**Upsert**

- All records of a run are written in one transaction. Any failure rolls back the whole batch, and the run fails with `Repository`.
- A failed run never leaves partial data.

**Record-level issues**

- Card or row without a name: skipped, counted, logged at `warn` with the position in the source.
- Field that cannot be parsed (invalid date, bad vote count): stored as `null`, logged at `warn` with the record id (FR6).
- Duplicate name in the Senado page: first occurrence kept, duplicates counted and logged at `info`.

**Lock not acquired**

- Not an error. Logged at `info`, the run is skipped.

**Scheduler loop**

- The loop body returns `Result`. Each error is logged and the loop continues to the next day's window. No error ends the loop.
- The run body is a separate function `run_daily_sync(...) -> Result` so it can be tested directly.
- If the scheduler task ends for any reason, `main.rs` logs `error` and keeps serving the API. Stored data remains readable.

**Stale data**

- If all sources keep failing, the API keeps serving the last stored data without an error to clients. Each failed run logs `error`. Surfacing staleness in the API is out of scope for v1.

### 14.4 Timeouts and retries

- HTTP clients: 30 second total timeout per request. One attempt per source per run.
- Database: sqlx pool default acquire timeout. No application-level retries.
- Startup connection failure: exit non-zero. The container restart policy handles retries.

### 14.5 HTTP error responses

| Condition | Status | Body |
|-----------|--------|------|
| `ListError::InvalidTerm` | 400 | `{"error": "invalid_term"}` |
| `ListError::Repository` | 500 | `{"error": "internal"}` |
| Unknown route | 404 | axum default, no custom body |

- The body is always `{"error": "<code>"}`. Codes are stable identifiers, never messages.
- Server errors are logged at `error` with the full error chain. Client errors are logged at `debug` without the query string value.

### 14.6 Log levels

| Level | Used for |
|-------|----------|
| `info` | Run start and end, source used, counts, lock skipped, duplicates removed |
| `warn` | Source failed and fallback used, skipped records, unparseable fields |
| `error` | Run failed, storage failure, startup failure, scheduler task ended |

### 14.7 Testing of error paths

Each variant in 14.2 has at least one test.

- Application: `AllSourcesFailed` when both fail; `Empty` treated as failure; fallback not called when primary succeeds; a fake repository error leaves no partial write.
- Parsers: missing container gives `Parse`; zero cards gives `Empty`; card without name is skipped and counted.
- Mapping: invalid JSON shape gives `Parse`; invalid date gives `None`, not an error.
- HTTP handler: 400 for invalid term; 500 body does not contain the internal message.
- Scheduler: `run_daily_sync` returns `Err` on failure and the loop logic continues (tested through the decision function from section 9).

## 15. Out of scope for this spec

- Frontend.
- Authentication and admin endpoints.
- Senator detail endpoint.
- Decoding obfuscated emails (D3).
- Retry with backoff beyond the single fallback attempt.
