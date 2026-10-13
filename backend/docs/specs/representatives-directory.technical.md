# Technical Spec: Representatives Directory

Status: WIP (pending approval)
Functional spec: `backend/docs/specs/representatives-directory.functional.md`
Scope: backend only
Mirrors: `backend/docs/specs/approved/senators-directory.technical.md`

## 1. Overview

The backend loads representantes of the Cámara de Representantes from two sources into PostgreSQL and serves them through `GET /api/representatives`.

| Term      | Source                                          | Access method                  | When                        |
|-----------|-------------------------------------------------|--------------------------------|-----------------------------|
| 2026-2030 | datos.gov.co `48i3-vuny` (primary)              | HTTP GET, Socrata SODA JSON    | Daily, 02:00-05:00 Bogotá   |
| 2026-2030 | Congreso Visible listing (fallback, secondary)  | HTTP GET, HTML parsing         | Only if the primary fails   |

The API never calls the external sources. Requests read only from PostgreSQL.

## 2. Verified source facts

Verified on 2026-10-11, not assumed.

### 2.1 datos.gov.co `48i3-vuny` (primary)

- Name: "ASISTENCIA HONORABLES REPRESENTANTES SESIONES PLENARIAS". Publisher: Cámara de Representantes. Updated 2026-09-18.
- Endpoint: `https://www.datos.gov.co/resource/48i3-vuny.json`. Returns a JSON array. No token required.
- Row count: 181 (`$select=count(*)`). Official seats: 183. The gap is explained in section 11, decision 3.
- Distinct `departamento` values: 47. Besides the 32 departments and Bogotá, the values include the special constituencies (for example `AFRODESCENDIENTES`, `INDIGENAS`, `CAMARA INTERNACIONAL`, `SAN ANDRES Y P. COMUNIDAD RAIZAL`) and the CITREP constituencies. Some values are malformed or combined (for example `CHOCO - ANTIOQUIA`, `undefined`). The mapping keeps the value as published.
- Field names used:
  - `sesion`: the representante's full name. The column name is misleading. It holds the name, not a session.
  - `partido_politico`: party, upper case (for example `ALIANZA VERDE`). CITREP rows use the list name (for example `CITREP 9- FUNDACIÓN ...`).
  - `departamento`: department or constituency of election, upper case (for example `NARIÑO`).
- Session columns (`sesi_n_plenaria_*`) hold `ASISTIO` or similar values. They change each time a session is added. The mapping ignores them.
- Values are all text. Placeholder `"ND"` is possible. The mapping normalizes it.
- Names are in the form `Apellido Apellido Nombre Nombre`, the same order as the senators.

Excluded datasets (functional spec, section 2):

- `5pt5-nxdp` ("REPRESENTANTES PERIODO 2024 - 2025"): 187 rows, but for the 2024-2025 period. Its columns do not match the data (`_` holds the name, `circunscripcion` holds emails, `apelidos_y_nombre` holds the department, and so on). It is not used.
- `7w3p-s9ve` (UTL staff): not representantes, and it includes document numbers. Not used.

### 2.2 Congreso Visible listing (fallback, secondary)

- Listing URL: `https://congresovisible.uniandes.edu.co/congresistas/?page=<n>&rows=<size>&search=&corporacion=1&cuatrienio=10`.
  - `corporacion=1`: Cámara de Representantes.
  - `cuatrienio=10`: the 2026-2030 term.
- Verified: the page returns 200 with 24 cards per request. The row selector offers up to 500 rows per page, so the client uses the largest size the site accepts and pages until a page has no profile links.
- Each card shows:
  - Surname(s) and given name(s) as two separate text blocks (for example `Abasolo Gómez` and `Alejandra Gabriela`). The full name is built as surnames then given names, which matches the datos.gov.co order.
  - Party or movement (for example `Avancemos Nariño`, `Cambio Radical`). Movements and coalitions are kept as published.
  - Commission: `Comisión: Sin comisión` for many members. Commission data is not used in v1 (functional spec, FR5).
- Each card links to a profile page `/congresistas/perfil/<slug>/<id>`. Profiles are not fetched in v1.
- Not published on the listing: constituency, email, phone, website, social media. Constituency comes from the primary source only. For records that come only from the fallback, `constituency` is `null`.
- This is a secondary source. Its `source` value is `congreso_visible`. Records from it are marked as such in the database and the response.

### 2.3 Official references (not used by the backend in v1)

Used for reconciliation (functional spec, section 7, item 1):

- CNE: declared the 2026-2030 list. Not reachable from the tools used during verification. No structured file found.
- Registraduría: the CITREP declarations confirm all 16 peace constituencies were declared. The escrutinio page is script-driven.

### 2.4 Historical terms

- No datos.gov.co dataset for the 2018-2022 Cámara members was found.
- Not in v1 (section 11, decision 2).

## 3. Architecture

Same hexagonal layout as the senators directory. Only the outbound adapters and the names change.

```
backend/
├── migrations/
│   └── 0002_create_representatives.sql
├── tests/
│   └── fixtures/
│       ├── datos_gov_co_representatives.json
│       └── congreso_visible_representatives.html
└── src/
    ├── main.rs                                 composition root (shared with senators)
    ├── domain/
    │   ├── representative.rs                   Representative, Gender, Term, RepresentativeId
    │   └── normalize.rs                        shared with senators (clean_text, normalize_name, ...)
    ├── application/
    │   ├── ports.rs                            RepresentativeSource, RepresentativeRepository (Clock shared)
    │   ├── sync_representatives.rs             SyncRepresentatives use case
    │   └── list_representatives.rs             ListRepresentatives use case
    └── adapters/
        ├── inbound/
        │   ├── http/
        │   │   └── representatives.rs          GET /api/representatives handler + DTO
        │   └── scheduler.rs                    shared with senators (one task per directory)
        └── outbound/
            ├── datos_gov_co/
            │   ├── client.rs                   shared client, dataset parameter (senators: sjwx-dr6n)
            │   └── mapping.rs                  senators mapping + representatives mapping
            ├── congreso_visible/
            │   ├── client.rs                   RepresentativeSource impl, paged HTTP calls
            │   └── parser.rs                   HTML listing -> Vec<Representative>
            └── postgres/
                └── representative_repository.rs
```

Notes:

- `domain::normalize` is shared with the senators. Nothing new is added unless a rule differs.
- `datos_gov_co::client` takes the dataset ID as a parameter, so the senators (`sjwx-dr6n`) and representatives (`48i3-vuny`) use the same HTTP code.
- Both directories share the scheduler. Each has its own time window and its own advisory lock key.
- `main.rs` is the only place that knows the concrete types.

## 4. Domain model

### 4.1 Types

```rust
pub enum Gender { Male, Female, Unknown }

pub enum SourceKind { DatosGovCo, Senado, Registraduria, CongresoVisible }

pub struct Term(String);            // "2026-2030", validated as YYYY-YYYY

pub struct Representative {
    pub id: RepresentativeId,       // derived, see 4.3
    pub term: Term,
    pub full_name: String,
    pub normalized_name: String,
    pub gender: Gender,             // always Unknown in v1
    pub birth_date: Option<chrono::NaiveDate>,
    pub department_of_birth: Option<String>,
    pub city_of_birth: Option<String>,
    pub party: Option<String>,
    pub constituency: Option<String>,
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

`updated_at` is set by the repository, the same as for senators.

`SourceKind` is shared by both directories. Representatives use `DatosGovCo` and `CongresoVisible` only. The senators use `DatosGovCo`, `Senado` and `Registraduria`. Each directory's migration restricts the `source` column to its own values (section 7).

### 4.2 Normalization rules

The rules from the senators technical spec (section 4.2) apply unchanged: `clean_text`, `normalize_name`, the `ND` and `N/D` placeholders, and `parse_votes`.

Additional rules for this directory:

| Function                   | Input example                        | Output                               |
|----------------------------|--------------------------------------|--------------------------------------|
| `clean_text(&str)`         | `"  NARIÑO "`                        | `Some("NARIÑO")` (case kept)         |
| `clean_text(&str)`         | `"ND"`, `""`, `"   "`                | `None`                               |
| `normalize_name(&str)`     | `"ABASOLO GÓMEZ ALEJANDRA GABRIELA"` | `"abasolo gomez alejandra gabriela"` |

### 4.3 Identity

- `normalized_name` = `normalize_name(full_name)`.
- `id` = `"{term}-{slug}"`, the same slug rule as the senators.
  - Example: `2026-2030-abasolo-gomez-alejandra-gabriela`.
- The pair `(term, normalized_name)` is unique in the database.
- The same person in both sources gives the same `id`. The upsert updates the stored record with the source that wrote it last.

### 4.4 Decision: contact and commission fields

Neither source publishes contact fields for the current term. Commission fields are not used in v1, even where the fallback shows them. All of these fields are `null` (functional spec, FR5 and FR6).

## 5. Application layer

### 5.1 Ports (`application/ports.rs`)

```rust
pub trait RepresentativeSource {
    fn kind(&self) -> SourceKind;
    fn fetch(&self, term: &Term) -> impl Future<Output = Result<Vec<Representative>, SourceError>> + Send;
}

pub trait RepresentativeRepository {
    fn last_updated(&self, term: &Term) -> impl Future<Output = Result<Option<DateTime<Utc>>, RepositoryError>> + Send;
    fn upsert_all(&self, representatives: &[Representative], now: DateTime<Utc>) -> impl Future<Output = Result<usize, RepositoryError>> + Send;
    fn list(&self, term: Option<&Term>) -> impl Future<Output = Result<Vec<Representative>, RepositoryError>> + Send;
    fn try_sync_lock(&self) -> impl Future<Output = Result<Option<SyncLock>, RepositoryError>> + Send;
}
```

`Clock` is shared with the senators. Native `async fn` in traits, generic use cases, no `async-trait`.

Errors reuse `SourceError`, `RepositoryError` and `SyncError` from the senators spec (section 14). There is no `has_term` method, because there is no historical load.

### 5.2 `SyncRepresentatives` use case

**`refresh_current(primary, fallback)`**

1. Try `primary.fetch("2026-2030")` (datos.gov.co).
2. If it fails (any `SourceError`, including `Empty`), log `warn` and try `fallback.fetch("2026-2030")` (Congreso Visible).
3. If the fallback also fails, return `SyncError::AllSourcesFailed`. The stored data is not touched.
4. On success, log `warn` if the count is below 183 (the official seat count), then `repo.upsert_all` in one transaction. A count below 183 does not fail the run.

**`is_stale(now, last_updated)`**: `true` if `last_updated` is `None` or older than 24 hours.

The fallback is `Option<S>`. Without a fallback, the use case runs with the primary source only.

### 5.3 `ListRepresentatives` use case

- `execute(term: Option<Term>) -> Vec<Representative>`. Delegates to `repo.list`.
- Term validation (`YYYY-YYYY`) happens here. An invalid format returns `ListError::InvalidTerm`, mapped to HTTP 400.
- Unknown but well-formed terms return an empty list.

## 6. Outbound adapters

### 6.1 datos.gov.co (`adapters/outbound/datos_gov_co`)

- `reqwest::Client` with a 30 second timeout and a `User-Agent` of `congreso-backend/<version>`.
- Request: `GET {base}/resource/48i3-vuny.json?$limit=1000`. The dataset holds only the current term, so there is no `$where` clause on the term.
- Optional token from `DATOS_GOV_CO_APP_TOKEN`, sent in the `X-App-Token` header when set. Never logged.
- `mapping.rs` converts each row:
  - `full_name` from `sesion`, via `clean_text`. `term` is always `2026-2030`.
  - `party` from `partido_politico`, via `clean_text`.
  - `constituency` from `departamento`, via `clean_text`.
  - `gender` is `Unknown`. Other fields are `None`.
  - Session columns are ignored.
- Rows without a usable name are skipped and counted in the log.
- Duplicate names within the dataset: the first occurrence is kept, the duplicates are counted and logged at `info`.

### 6.2 Congreso Visible (`adapters/outbound/congreso_visible`)

Client:

- Requests the listing URL in section 2.2, page by page, starting at `page=1`.
- Stops when a page has no profile links, or when the number of records reaches the total shown on the listing.
- Same timeout and `User-Agent` as the datos.gov.co client, with one request per page.
- One logical request per run: paging is part of the same run, not a retry.
- Non-2xx status becomes `SourceError::UnexpectedStatus`.

Parser (`parser.rs`, `scraper` crate):

1. Select the profile cards (links to `/congresistas/perfil/`).
2. For each card:
   - `full_name` = surname text + given-name text, trimmed and whitespace-collapsed. Empty name: skip the card and count it.
   - `party` = the party or movement text, `clean_text`.
   - `constituency`: `None` (not on the listing).
   - `source` = `CongresoVisible`. `gender` = `Unknown`. All other fields `None`, including commission fields (section 4.4).
3. Deduplicate by `normalized_name`, keeping the first occurrence. Log the number of duplicates removed.
4. If the result is empty, return `SourceError::Empty`.
5. Missing listing container: `SourceError::Parse`.

Parser tests use `tests/fixtures/congreso_visible_representatives.html`, a trimmed copy of one listing page with at least one card from each of the following: a multi-word surname, a name with an accent, a party and a movement, and a card with the commission text `Sin comisión`. It also includes one duplicate name.

### 6.3 PostgreSQL (`adapters/outbound/postgres`)

- Same `sqlx::PgPool` and migration approach as the senators.
- `upsert_all`: one transaction, `INSERT ... ON CONFLICT (id) DO UPDATE SET <all columns>, updated_at = EXCLUDED.updated_at`. Any error rolls back the whole batch.
- `try_sync_lock`: `pg_try_advisory_lock` on a key different from the senators' key.
- `list`: `SELECT ... FROM representatives [WHERE term = $1] ORDER BY term DESC, full_name`.
- `last_updated`: `SELECT MAX(updated_at) FROM representatives WHERE term = $1`.

## 7. Migrations

`backend/migrations/0002_create_representatives.sql`:

```sql
CREATE TABLE representatives (
    id                          TEXT PRIMARY KEY,
    term                        TEXT NOT NULL,
    normalized_name             TEXT NOT NULL,
    full_name                   TEXT NOT NULL,
    gender                      TEXT NOT NULL CHECK (gender IN ('male', 'female', 'unknown')),
    birth_date                  DATE,
    department_of_birth         TEXT,
    city_of_birth               TEXT,
    party                       TEXT,
    constituency                TEXT,
    votes                       BIGINT CHECK (votes IS NULL OR votes >= 0),
    constitutional_commission   TEXT,
    legal_commission            TEXT,
    email                       TEXT,
    phone                       TEXT,
    phone_extension             TEXT,
    website                     TEXT,
    twitter                     TEXT,
    facebook                    TEXT,
    source                      TEXT NOT NULL CHECK (source IN ('datos_gov_co', 'congreso_visible')),
    updated_at                  TIMESTAMPTZ NOT NULL,
    UNIQUE (term, normalized_name)
);

CREATE INDEX representatives_term_idx ON representatives (term);
```

The schema matches the senators table, with `constituency` added and the `source` values changed. It is a separate table, so the two directories do not share rows.

## 8. HTTP (inbound)

### 8.1 Endpoint

`GET /api/representatives[?term=YYYY-YYYY]`

- `200 OK` with a JSON array.
- `400 Bad Request` if `term` is present and not `YYYY-YYYY`. Body: `{"error": "invalid_term"}`.
- `500 Internal Server Error` on database failure. Body: `{"error": "internal"}`.

### 8.2 Response DTO

```json
{
  "id": "2026-2030-abasolo-gomez-alejandra-gabriela",
  "term": "2026-2030",
  "full_name": "ABASOLO GÓMEZ ALEJANDRA GABRIELA",
  "gender": "unknown",
  "birth_date": null,
  "department_of_birth": null,
  "city_of_birth": null,
  "party": "AVANCEMOS NARIÑO",
  "constituency": "NARIÑO",
  "votes": null,
  "constitutional_commission": null,
  "legal_commission": null,
  "email": null,
  "phone": null,
  "phone_extension": null,
  "website": null,
  "twitter": null,
  "facebook": null,
  "source": "datos_gov_co",
  "updated_at": "2026-10-11T07:00:00Z"
}
```

The DTO is a separate type in `adapters/inbound/http/representatives.rs`. The domain type is not serialized directly. The example uses a record from the primary source. A record from the fallback has `"source": "congreso_visible"` and `"constituency": null`.

## 9. Scheduler (inbound)

`adapters/inbound/scheduler.rs`, shared with the senators. In-process task, as in the senators spec. No external cron.

**Startup**

1. If `last_updated("2026-2030")` is stale, run `refresh_current` once.
2. Start the daily loop.

**Daily loop**

1. Compute the next run as a random instant between 02:00 and 05:00 in `America/Bogota`.
2. Sleep until that instant.
3. Acquire the representatives' advisory lock. If not acquired, log and skip.
4. Run `refresh_current`. Log the outcome (source used, count, duration).
5. Go back to step 1.

The window, the lock key and the use case are specific to each directory. The loop code is shared.

## 10. Configuration and dependencies

No new environment variables. The senators spec's variables apply (`DATABASE_URL`, `BIND_ADDRESS`, `DATOS_GOV_CO_APP_TOKEN`, `RUST_LOG`).

No new crates. The HTTP, HTML, date and error crates from the senators spec cover this directory.

## 11. Decisions

Status: WIP. Requires approval with the functional spec.

1. **Fallback source. Decided: Congreso Visible, secondary.** The official sources checked (Cámara website, CNE, Registraduría, datos.gov.co) provide no structured 2026-2030 list the backend can read (functional spec, section 7, item 1). Congreso Visible is reachable and gives name, party and commission. Its records are stored with `source = congreso_visible`. The CNE list is kept as the reconciliation reference.
2. **Historical term 2018-2022. Decided: not in v1.** No datos.gov.co source was found. This matches the senators spec's rule of not adding terms the sources do not publish.
3. **Seat gap (181 of 183). Decided: store and log.** The primary source has 15 of the 16 CITREP seats. CITREP 2 is missing, although the Registraduría declared all 16. The second missing seat cannot be identified from the rows alone. Both gaps are logged on every run (section 5.2 step 4). Reconciliation against the CNE list is a follow-up, not a v1 blocker.
4. **Timezone. Decided: `America/Bogota`,** as in the senators spec.
5. **Multiple instances. Decided: advisory lock,** as in the senators spec, with a separate key.
6. **Commissions in v1. Decided: `null`.** The Cámara commission pages describe the previous term. Congreso Visible shows `Sin comisión` for many members. Revisit when commission data for 2026-2027 is available.
7. **Scheduler. Decided: in-process task,** as in the senators spec.

## 12. Testing strategy

Same targets as the senators spec (section 12):

| Area                                      | Test type                 | Coverage target |
|-------------------------------------------|---------------------------|-----------------|
| `domain::normalize`                       | Unit, table-driven        | 100% of branches |
| `application::sync_representatives`       | Unit, fake source/repo/clock | Full (fallback, empty, skip, stale, count below 183) |
| `application::list_representatives`       | Unit, fake repo           | Full            |
| `datos_gov_co::mapping` (representatives) | Unit, JSON fixture        | Full, including session columns ignored |
| `congreso_visible::parser`                | Unit, HTML fixture        | Full, including `Sin comisión` and duplicates |
| `congreso_visible::client` paging         | Unit, stubbed pages       | Full (stops on empty page) |
| `http::representatives` handler           | Unit, `tower::ServiceExt::oneshot` with fake repo | Full |
| `postgres::representative_repository`     | Integration, `TEST_DATABASE_URL` | Full when DB available |

No test performs a live network call. The datos.gov.co fixture is three real rows from `48i3-vuny`, plus one row with `ND` and one row with a name in a different case.

## 13. Acceptance mapping

| Functional acceptance criterion                           | Technical coverage                         |
|-----------------------------------------------------------|--------------------------------------------|
| 1. Endpoint returns 2026-2030 records with FR5 fields     | 8.2 DTO, sync use case, integration test   |
| 2. `?term=2026-2030` returns only current term            | 5.3, repository `list`                     |
| 3. datos.gov.co down, endpoint serves stored data         | 5.2 (stored data not touched), 8.1         |
| 4. Fewer than 183 stored and logged                       | 5.2 step 4, 6.1                            |
| 5. Running the daily job twice creates no duplicates      | 6.3 upsert on `id`                         |
| 6. Migrations create schema on empty PostgreSQL 17        | 7, migration runs at startup               |
| 7. `cargo test` passes, `cargo llvm-cov` at least 85%     | 12                                         |

## 14. Error handling

Same as the senators spec (section 14), with these differences:

- The Congreso Visible fallback uses the same `SourceError` variants. Paging errors mid-run count as a source failure for that run. The run keeps the stored data.
- The count check in 5.2 step 4 logs at `warn`. It does not fail the run. Only a source failure or an empty result triggers the fallback.
- Log levels follow section 14.6 of the senators spec.

## 15. Out of scope for this spec

- Frontend.
- Authentication and admin endpoints.
- Representante detail endpoint, including the Congreso Visible profile pages.
- Historical terms.
- Attendance data from `48i3-vuny`.
- Document numbers and other identity data (never stored).
- Commission data (decision 6).
