# Functional Spec: Representatives Directory

Status: WIP (pending approval)
Scope: backend only
Language: API fields in English. User-facing text in Spanish, where applicable.
Mirrors: `backend/docs/specs/approved/senators-directory.functional.md`

## 1. Goal

The backend exposes a JSON list of the representantes (members) of the Cámara de Representantes of Colombia for the current term, 2026-2030.

The API serves data from PostgreSQL. It never calls the external sources while handling a request.

Historical terms are out of scope for v1 (see section 2, Notes).

## 2. Data sources

| Term      | Source                                                                                    | Role                         | Refresh                 |
|-----------|-------------------------------------------------------------------------------------------|------------------------------|-------------------------|
| 2026-2030 | datos.gov.co, dataset `48i3-vuny` (SODA API, `resource/48i3-vuny.json`)                  | Primary                      | Daily, 02:00-05:00      |
| 2026-2030 | Congreso Visible (Universidad de los Andes), listing `congresovisible.uniandes.edu.co/congresistas/?corporacion=1&cuatrienio=10` (HTML, paginated up to 500 rows) | Fallback (proposed, not official) | When primary fails      |
| 2018-2022 | None found on datos.gov.co                                                                | Not in v1                    | Not applicable          |

Notes:

- `48i3-vuny` is the Cámara's attendance dataset for the current term. It lists each representante with party and department, and it is updated monthly during the term. At verification time (2026-10-11) it held 181 rows, against 183 seats.
- The 2026-2030 list on the Cámara website comes from the Consejo Nacional Electoral (CNE). It is the official list. The fallback source is not verified yet (see technical spec, section 2.3).
- Excluded sources:
  - `5pt5-nxdp` ("REPRESENTANTES PERIODO 2024 - 2025"): covers the 2024-2025 period, not the current term. Its columns are shifted (`_` holds the name, `circunscripcion` holds emails, and so on), so it cannot be mapped reliably.
  - `7w3p-s9ve` (UTL staff): lists staff of the representantes' units, not the representantes. It also includes identity document numbers, which the backend must not store.
  - `vkjr-c6fe` (2018 election results) and `xk84-sret` (Sabaneta municipal results): election results, not a directory.
- No datos.gov.co dataset was found for the 2018-2022 Cámara members. The historical load of the senators spec (`sjwx-dr6n`) has no equivalent here. Per the senators spec, section 5, terms the sources do not publish are not added. This is a recorded limitation.
- The sources do not publish contact data (email, phone, website) for the current term. These fields are stored as `null`.

## 3. Functional requirements

### FR1 Persistence

- Representantes are stored in PostgreSQL 17.
- The backend accesses PostgreSQL through `sqlx`, including the SQL migrations.
- The API reads only from the database.

### FR2 Scheduled refresh

- A background job refreshes the current-term data once per day.
- The daily run starts at a random time between 02:00 and 05:00 (America/Bogota), to spread load.
- The job runs once at application startup if the current-term data is missing or stale (older than 24 hours).
- The job upserts records by identity (see FR7). It does not delete records that are missing from a run, unless the source confirms the representante is no longer listed.

### FR3 Fallback and failure handling

- If the datos.gov.co source fails (network error, non-success HTTP status, or no representantes parsed), the job tries the Cámara website source.
- If both sources fail, the job keeps the last successfully stored data unchanged and logs the error.
- A failed run never leaves the database empty or partially updated. Each run is applied in a single transaction.
- Sanity check: if a run returns fewer than 183 representantes, the run still succeeds, but the job logs a warning with the count. The official count is 183 seats.

### FR4 List endpoint

- `GET /api/representatives` returns a JSON array of representantes.
- Optional query parameter `term` filters by term, for example `term=2026-2030`.
- An unknown `term` value returns an empty array, not an error.
- When no data has been loaded yet, the endpoint returns an empty array.

### FR5 Output fields

Each representante in the response contains:

| Field                       | Type            | Notes                                                                 |
|-----------------------------|-----------------|-----------------------------------------------------------------------|
| `id`                        | string          | Stable identifier (see FR7)                                           |
| `term`                      | string          | `2026-2030` for v1                                                    |
| `full_name`                 | string          | As published by the source, trimmed. Source is upper case, see FR6.  |
| `gender`                    | string          | `unknown` for v1 (no source publishes gender)                         |
| `birth_date`                | string or null  | `null` for v1                                                         |
| `department_of_birth`       | string or null  | `null` for v1                                                         |
| `city_of_birth`             | string or null  | `null` for v1                                                         |
| `party`                     | string or null  | From `partido_politico`                                               |
| `constituency`              | string or null  | Department the representante was elected in (`departamento`). New field, not in the senators' output. |
| `votes`                     | integer or null | `null` for v1                                                         |
| `constitutional_commission` | string or null  | `null` for v1 (see open item 2)                                       |
| `legal_commission`          | string or null  | `null` for v1 (see open item 2)                                       |
| `email`                     | string or null  | `null` for v1 (see FR6)                                               |
| `phone`                     | string or null  | `null` for v1                                                         |
| `phone_extension`           | string or null  | `null` for v1                                                         |
| `website`                   | string or null  | `null` for v1                                                         |
| `twitter`                   | string or null  | `null` for v1                                                         |
| `facebook`                  | string or null  | `null` for v1                                                         |
| `source`                    | string          | `datos_gov_co` (primary) or `congreso_visible` (fallback)            |
| `updated_at`                | string          | ISO 8601 timestamp of the last stored update                          |

The field set matches the senators' output, so one frontend can render both directories. The only addition is `constituency`.

### FR6 Normalization rules

- Placeholder values `ND`, `N/D`, empty strings and whitespace-only strings become `null`.
- Names in the source are upper case (for example `ABASOLO GÓMEZ ALEJANDRA GABRIELA`). They are trimmed and whitespace-collapsed. Case is kept as published, so the API returns the upper-case form.
- Party and department names are trimmed and whitespace-collapsed. Case is preserved.
- Gender is `unknown` for every record in v1.
- Contact fields are `null` for the current term.
- A record is rejected only if it has no name. Other invalid fields do not reject the record.
- Session attendance columns in `48i3-vuny` are ignored. Their names change each month.

### FR7 Identity

- A representante is identified by the pair `(term, normalized full_name)`.
- Normalized full name: trimmed, whitespace collapsed, lowercase, and accents removed.
- The public `id` is derived from that pair, so it stays stable across runs.
- The same person in two terms is two records, one per term. They are not merged.

## 4. Non-functional requirements

- **Testing:** unit tests are mandatory. Line coverage must be at least 85%, measured with `cargo llvm-cov`.
- **No network in unit tests:** parsers and normalizers are tested with recorded fixtures. Source clients are tested against stubbed responses.
- **Politeness:** the client sends an identifying `User-Agent`, makes one request per run per source, and uses timeouts.
- **Security:** no credentials in code. `DATABASE_URL` and any optional source token come from environment variables.
- **Architecture:** hexagonal layout from AGENTS.md. Domain has no framework or SQL imports. HTTP handlers stay thin.
- **Observability:** each run logs its start, end, source used, record counts and errors.
- **Database:** schema is created by SQL migrations managed by `sqlx`.

## 5. Out of scope

- Frontend pages.
- Authentication, roles or admin endpoints.
- Representante detail endpoint (may be added later).
- Historical terms (2018-2022 and earlier) until a source is found.
- Attendance data from `48i3-vuny`.
- Manual data editing.
- Permission or licensing review of the Cámara website (same status as the senators spec).

## 6. Acceptance criteria

1. `GET /api/representatives` returns the 2026-2030 records, with the fields in FR5.
2. `GET /api/representatives?term=2026-2030` returns only current-term records.
3. When datos.gov.co is unreachable, the endpoint still returns the last stored data, and the logs show the fallback attempt.
4. A run with fewer than 183 representantes stores the records it parsed and logs the count.
5. Running the daily job twice does not create duplicate records.
6. Migrations create the schema on an empty PostgreSQL 17 database.
7. `cargo test` passes and `cargo llvm-cov` reports at least 85% line coverage.

## 7. Open items: resolved

Checked on 2026-10-11 against datos.gov.co and the web.

1. **Cámara website list: markup and selectors. Answer: no parseable list in the page.**
   - The article `camara.gov.co/representantes-a-la-camara-elegidos-para-el-periodo-legislativo-2026-2030/` (published 2026-04-07, data from the CNE) has no table, no link to a list file, and no embedded list. Its only PDF links are the terms-of-use documents. The embedded viewer uses a template placeholder (`${urlPDF}`), so the list is loaded or attached by something outside the HTML.
   - Consequence: the camara.gov.co page cannot be the fallback as specified.
   - Follow-up search (2026-10-11), looking for another source:
     - **CNE** is the official body that declared the 2026-2030 list (El Nuevo Siglo, 30 March 2026, "listado oficial"; El Espectador, "lista completa ... tras los escrutinios"). The CNE site could not be reached from the tools used here (transport error), and no structured file was found. It stays the official reference for reconciliation, pending access.
     - **Registraduría:** the escrutinio page (`wapp.registraduria.gov.co/electoral/atipicas/congreso/escrutinios.html`) renders no list in its HTML. Its CSV archive (cedae.datasketch.co) stops at 2019.
     - **datos.gov.co:** no 2026 election, CNE or Registraduría dataset with winners.
     - **Congreso Visible (Universidad de los Andes): usable.** Its 2026-2030 Cámara listing returns name, party/movement and commission per member, with paginated HTML and one profile page per member. It is a secondary source, not official.
     - **Wikipedia (es):** the 2026-2030 Congress article lists the members and cites the CNE, but it is a secondary source. Use it for reconciliation only.
   - Recommendation: Congreso Visible as the proposed fallback, with `source = congreso_visible` (new value in the migration), flagged as secondary. Reconcile against the CNE list once it can be reached. Note: Congreso Visible's commission column shows "Sin comisión" for many members, so commission data from it is not reliable yet.
   - Third-party lists (El Espectador, Radio Nacional, Focus Noticias PDF) contain the full list. They can be used to reconcile names, but they are not sources for the backend.

2. **The 2 missing seats (181 published, 183 seats). Answer: at least one is a source gap, not a vacancy.**
   - 183 seats = 161 territorial + 16 CITREP (peace constituencies) + 2 Afro-descendant + 1 Indigenous + 1 Raizal + 1 colombianos en el exterior (Radio Nacional, Wikipedia).
   - The dataset `48i3-vuny` has 15 of the 16 CITREP rows. CITREP 2 is missing. The Registraduría states that all 16 CITREP were declared (`registraduria.gov.co/Se-declaro-la-eleccion-de-las-16-curules-de-paz-y-de-la-Camara-de.html`), so CITREP 2 exists and is a source gap.
   - The second missing seat cannot be identified from the rows alone. El Espectador's list (183 names) is the reference to compare against once a fallback is chosen. Press reported that Chocó and Cundinamarca were still pending escrutinio at one point. That may explain a gap at the time the dataset was built.
   - Decision: store the 181 rows and log the gap (FR3). Reconcile by name against the CNE/Registraduría list once it is confirmed.

3. **Commissions for v1. Answer: no.**
   - The camara.gov.co commission pages (for example `integrantes-comision-primera-constitucional-permanente`) were last modified 2026-01-29, before the 2026-2030 term started on 2026-07-20. They describe the previous term.
   - `5pt5-nxdp` has commission-like fields, but it covers 2024-2025 and its columns are misaligned (section 2).
   - Decision: commission fields stay `null` in v1 (FR5). Revisit when the commission pages list the 2026-2027 legislature.

4. **Scheduler implementation. Answer: in-process task, as in the senators spec.**
   - This matches the approved senators technical spec (section 9) and AGENTS.md, which asks for minimal additions. Adding a cron job would add a container or host dependency.
   - The advisory lock (technical spec, section 6.3) covers the case where more than one backend instance runs. The compose file runs one backend.
   - Decision: in-process, same as the senators.
