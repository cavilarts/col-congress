# AGENTS.md

Context for LLM coding agents working in this repository. Read this before making changes.

## Project purpose

Web application that will connect to Colombia's public government APIs (open data from the Colombian government). The current state is boilerplate only: no government API integration exists yet.

## Layout

```
congreso/                 git root (branch: main, remote: git@github.com:cavilarts/col-congress.git)
├── compose.yaml          Podman/Docker compose: postgres, backend, frontend
├── README.md             Human-facing documentation
├── AGENTS.md             This file
├── backend/              Rust API (Axum), hexagonal architecture
└── frontend/             Rust web UI (Leptos 0.8 SSR + hydrate), Tailwind v4
```

## Stack

- Language: Rust, edition 2024 (both crates).
- Backend: `axum` 0.8, `tokio` 1 (`macros`, `rt-multi-thread`).
- Frontend: `leptos` 0.8, `leptos_axum` 0.8, `axum` 0.8 (optional, `ssr`), `wasm-bindgen`, `console_error_panic_hook`. Build tool: `cargo-leptos` 0.3.11. Styling: Tailwind CSS v4 via cargo-leptos (`style/tailwind.css` contains `@import "tailwindcss";`).
- Database: PostgreSQL 17 (container image `docker.io/library/postgres:17`). No database driver is in the backend yet.
- Containers: Podman, using `Containerfile` files (not `Dockerfile`) and `compose.yaml`.

## Architecture

### Backend (`backend/`)

Hexagonal (ports and adapters). Keep the dependency direction inward:

- `src/domain/`: entities, value objects and domain rules. Must not depend on Axum, Tokio, SQL or HTTP code. Empty so far.
- `src/application/`: use cases and ports (traits describing what the app needs from the outside, such as repositories or API clients). Depends only on `domain`. Empty so far.
- `src/adapters/inbound/http/`: driving adapter. Axum router and handlers. Currently exposes `GET /health` returning `ok`. Converts HTTP requests into application calls.
- `src/adapters/outbound/` (not created yet): driven adapters, e.g. PostgreSQL repositories and HTTP clients for government APIs. Implement the ports from `application/`.
- `src/main.rs`: composition root. Builds the router from `adapters::inbound::http::router()` and listens on `0.0.0.0:3000`. Wire dependencies here when adding adapters.

The `adapters/mod.rs`, `adapters/inbound/mod.rs` and module declarations use `pub mod` so the tree is visible from `main.rs`.

### Frontend (`frontend/`)

- `src/main.rs`: SSR server, compiled only with feature `ssr`. Uses `leptos_axum` routes and `file_and_error_handler`. Reads config through `get_configuration(None)` (cargo-leptos `[package.metadata.leptos]` values, overridable by `LEPTOS_*` env vars).
- `src/app.rs`: `shell(options)` HTML document (feature `ssr`) and the `App` component.
- `src/lib.rs`: exports `app` module and the `hydrate()` wasm entry point (feature `hydrate`).
- `style/tailwind.css`: Tailwind input. Add custom CSS here only if needed.
- Ports: site `127.0.0.1:3001` locally, reload `3002` (cargo-leptos reserves `port+1` by default, so reload was set explicitly to avoid a conflict).

## Services and ports

| Service  | Container port | Host port | Notes                                              |
|----------|----------------|-----------|----------------------------------------------------|
| backend  | 3000           | 3000      | `/health`                                          |
| frontend | 3001           | 3001      | SSR + hydration                                    |
| postgres | 5432           | 5433      | Host 5432 is often used by a local PostgreSQL      |

Credentials and DB name default to `congreso`, overridable with `POSTGRES_USER`, `POSTGRES_PASSWORD`, `POSTGRES_DB`. Named volume: `postgres_data`. Backend `depends_on: postgres`, but no `DATABASE_URL` is set yet.

## Commands

Run from the directory indicated.

```sh
# Backend (backend/)
cargo check
cargo run                          # http://localhost:3000
cargo test                         # no tests yet

# Frontend (frontend/)
cargo leptos build                 # dev build (SSR bin + wasm + Tailwind)
cargo leptos build --release
cargo leptos serve                 # http://127.0.0.1:3001
cargo leptos watch                 # dev server with reload on port 3002

# Containers (repo root)
podman compose up --build -d
podman compose ps
podman compose logs -f
podman compose down                # add -v to delete the postgres volume
```

Use `podman compose`, not `podman-compose`: the Homebrew `podman-compose` package delegates to the Docker compose plugin here.

## Build and environment gotchas

- `cargo install cargo-leptos` fails on Rust 1.95 because `cargo-util-schemas` 0.14.2 needs rustc 1.96. Use the prebuilt release binary (v0.3.11) instead. The frontend `Containerfile` already downloads it from GitHub. Assets are named `cargo-leptos-<arch>-unknown-linux-gnu.tar.gz` for Linux and `cargo-leptos-<arch>-apple-darwin.tar.gz` for macOS.
- The frontend requires the `wasm32-unknown-unknown` target (`rustup target add wasm32-unknown-unknown`).
- The Podman VM needs at least 8 GB memory; with 2 GB the frontend release build is SIGKILLed.
- Docker Hub pulls can fail intermittently with "unauthorized". Retry, or `podman pull` the image first.
- `frontend/Cargo.toml` uses `reload-port = 3002` because the default reload port (3001) equals the site port.
- Package and binary names: backend crate `backend`, frontend crate `frontend` (lib + bin). The frontend binary is `target/release/frontend`; the site assets go to `target/site` (`pkg/frontend.css`, `pkg/frontend.js`, `pkg/frontend.wasm`).
- Each crate has its own `.gitignore` (`/target`) and `.dockerignore` (`target`). Keep them when touching build setup.

## Conventions and constraints

- The project owner asked for minimal boilerplate: add only what is needed for the current request. Do not add frameworks, libraries, folders or features speculatively.
- Put new code in the correct hexagonal layer. Domain code must not import Axum, Tokio, SQL or HTTP types.
- Keep HTTP handlers thin; move logic into the application layer.
- Use `Containerfile` names for container builds and reference them explicitly in `compose.yaml` (`dockerfile: Containerfile`).
- The UI language is Spanish (`<html lang="es">`) and the domain is Colombian public data. Keep user-facing text consistent with that.
- Don't commit unless the user asks. When asked to commit, check `git status` first and don't add `target/` or secrets. Use concise commit messages that describe what changed.
- Don't rewrite git history or force-push.
- Don't hardcode credentials. Database defaults in `compose.yaml` are development-only.
- Don't install system software without asking. The Podman machine and `cargo-leptos` were installed in this project's setup on request.

## Current state (as of the initial commit `47aa60c`)

- Backend: `/health` endpoint only. Empty `domain` and `application` modules.
- Frontend: placeholder page with the title "Congreso" styled with Tailwind classes.
- Compose: postgres, backend and frontend build and run. The backend does not use the database yet.
- Remote: `origin` at `git@github.com:cavilarts/col-congress.git`, pushed to `main`.

## Next likely steps

1. Define domain entities and application ports for the government data sources.
2. Add a PostgreSQL adapter (SQL driver, migrations, `DATABASE_URL` in compose).
3. Add outbound HTTP adapters for Colombian public APIs.
4. Add backend API routes and frontend pages that consume them.
