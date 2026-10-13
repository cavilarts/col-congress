# Functional Spec: Senators Directory

Status: WIP (pending approval)
Scope: backend only
Language: API fields in English. User-facing text in Spanish, where applicable.

## 1. Goal

The backend exposes a JSON list of the senators of the Republic of Colombia, covering:

- The 2018-2022 term, from the official open dataset on datos.gov.co.
- The current 2026-2030 term, from the Senado de la República website.

The API serves data from PostgreSQL. It never calls the external sources while handling a request.

## 2. Data sources

| Term      | Source                                                                 | Role                        | Refresh            |
|-----------|------------------------------------------------------------------------|-----------------------------|--------------------|
| 2018-2022 | datos.gov.co, dataset `sjwx-dr6n` (SODA API, `resource/sjwx-dr6n.json`) | Historical snapshot         | Once, at first run |
| 2026-2030 | Senado website, `https://www.senado.gov.co/index.php/el-senado/senadores` | Primary for current term    | Daily, 02:00-05:00 |
| 2026-2030 | Registraduría election results                                         | Fallback if the Senado fails | When primary fails |

Notes:

- The 2018-2022 dataset contains 106 rows. The 2022-2026 Senate had 108 members, so this dataset may be incomplete. This is recorded as a known limitation, not corrected.
- The Senado website does not expose emails in plain text. Emails for the current term are stored as `null`.

## 3. Functional requirements

### FR1 Persistence

- Senators are stored in PostgreSQL 17.
- The backend accesses PostgreSQL through `sqlx`, including the SQL migrations.
- The API reads only from the database.

### FR2 Scheduled refresh

- A background job refreshes the current-term data once per day.
- The daily run starts at a random time between 02:00 and 05:00 (server local time), to spread load and avoid a fixed hour.
- The job runs once at application startup if the current-term data is missing or stale (older than 24 hours).
- The job upserts records by identity (see FR7). It does not delete records that are missing from a run, unless the source confirms the senator is no longer listed.
- The 2018-2022 dataset is loaded once, on the first run, and skipped on later runs if already present.

### FR3 Fallback and failure handling

- If the Senado website fails (network error, non-success HTTP status, or no senators parsed), the job tries the Registraduría source.
- **v1 status:** no structured Registraduría source was found (technical spec, section 11, decision 1). The fallback is disabled in v1, so this step is skipped.
- If both sources fail, the job keeps the last successfully stored data unchanged and logs the error.
- A failed run never leaves the database empty or partially updated. Each run is applied in a single transaction.

### FR4 List endpoint

- `GET /api/senators` returns a JSON array of senators.
- Optional query parameter `term` filters by term, for example `term=2026-2030`.
- An unknown `term` value returns an empty array, not an error.
- When no data has been loaded yet, the endpoint returns an empty array.

### FR5 Output fields

Each senator in the response contains:

| Field                      | Type            | Notes                                                    |
|----------------------------|-----------------|----------------------------------------------------------|
| `id`                       | string          | Stable identifier (see FR7)                              |
| `term`                     | string          | For example `2018-2022` or `2026-2030`                   |
| `full_name`                | string          | As published by the source, trimmed                      |
| `gender`                   | string          | `male`, `female` or `unknown`                            |
| `birth_date`               | string or null  | ISO 8601 date (`YYYY-MM-DD`) or `null`                   |
| `department_of_birth`      | string or null  |                                                          |
| `city_of_birth`            | string or null  |                                                          |
| `party`                    | string or null  |                                                          |
| `votes`                    | integer or null | Parsed from text such as `"82,854"`                      |
| `constitutional_commission`| string or null  |                                                          |
| `legal_commission`         | string or null  |                                                          |
| `email`                    | string or null  | `null` for the current term (see FR6)                    |
| `phone`                    | string or null  |                                                          |
| `phone_extension`          | string or null  |                                                          |
| `website`                  | string or null  |                                                          |
| `twitter`                  | string or null  |                                                          |
| `facebook`                 | string or null  |                                                          |
| `source`                   | string          | `datos_gov_co`, `senado` or `registraduria`              |
| `updated_at`               | string          | ISO 8601 timestamp of the last stored update             |

### FR6 Normalization rules

- Placeholder values `ND`, `N/D`, empty strings and whitespace-only strings become `null`.
- Gender values are matched case-insensitively and mapped to `male`, `female` or `unknown`.
- Vote counts are parsed by removing thousands separators. Unparseable values become `null`.
- Birth dates from the dataset are free text in Spanish (for example `25 de marzo de 1971`). They are parsed into dates. Values that cannot be parsed (for example `22 de noviembre de 1066`) become `null` and are logged with the record id.
- Department, city, party and commission names are trimmed and whitespace-collapsed. Case is preserved.
- Emails for the current term are not stored and are returned as `null`.
- A record is rejected only if it has no name. Other invalid fields do not reject the record.

### FR7 Identity

- A senator is identified by the pair `(term, normalized full_name)`.
- Normalized full name: trimmed, whitespace collapsed, lowercase, and accents removed.
- The public `id` is derived from that pair (for example a deterministic slug), so it stays stable across runs.
- The same person in two terms is two records, one per term. They are not merged.

## 4. Non-functional requirements

- **Testing:** unit tests are mandatory. Line coverage must be at least 85%, measured with `cargo llvm-cov`.
- **No network in unit tests:** parsers and normalizers are tested with recorded fixtures. Source clients are tested against stubbed responses.
- **Politeness:** the scraper sends an identifying `User-Agent`, makes one request per run per source, and uses timeouts.
- **Security:** no credentials in code. `DATABASE_URL` and any optional source token come from environment variables.
- **Architecture:** hexagonal layout from AGENTS.md. Domain has no framework or SQL imports. HTTP handlers stay thin.
- **Observability:** each run logs its start, end, source used, record counts and errors.
- **Database:** schema is created by SQL migrations managed by `sqlx`. An empty database becomes ready after running migrations.

## 5. Out of scope

- Frontend pages.
- Authentication, roles or admin endpoints.
- Senator detail endpoint (may be added later).
- Terms before 2018-2022 or after 2026-2030 beyond what the sources publish.
- Manual data editing.
- Permission or licensing review of the Senado website (decided: skipped for now).

## 6. Acceptance criteria

1. `GET /api/senators` returns the 2018-2022 records and the 2026-2030 records, with the fields in FR5.
2. `GET /api/senators?term=2026-2030` returns only current-term records.
3. When the Senado website is unreachable, the endpoint still returns the last stored data, and the logs show the fallback attempt.
4. A run with an unparseable birth date stores the record with `birth_date: null` and logs it.
5. Running the daily job twice does not create duplicate records.
6. Migrations create the schema on an empty PostgreSQL 17 database.
7. `cargo test` passes and `cargo llvm-cov` reports at least 85% line coverage.

## 7. Open items for the technical spec

- Exact HTML selectors and parsing approach for the Senado page.
- Name matching rules between the dataset and the Senado page, if the same person appears in both (they are separate terms, so this is likely not needed).
- Behavior when a senator disappears from the current-term list (FR2 currently keeps them).
- Scheduler implementation (in-process task with a time window, or an external cron).
