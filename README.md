# Congreso

Web application that connects to Colombia's public government APIs. It is split into:

- **backend**: Rust web API built with [Axum](https://github.com/tokio-rs/axum), organized with hexagonal architecture.
- **frontend**: server-rendered web UI built with [Leptos](https://leptos.dev) 0.8 and styled with [Tailwind CSS](https://tailwindcss.com) v4.
- **postgres**: PostgreSQL 17 database.

Everything runs locally with [Podman](https://podman.io) via `compose.yaml`.

> Status: project boilerplate. The backend exposes only a health check, the frontend renders a placeholder page, and no integration with government APIs has been implemented yet.

## Repository layout

```
.
├── compose.yaml          # Orchestrates postgres, backend and frontend
├── backend/              # Axum API (hexagonal architecture)
│   ├── Cargo.toml
│   ├── Containerfile     # Multi-stage image build
│   └── src/
│       ├── main.rs       # Starts the HTTP server on port 3000
│       ├── domain/       # Business entities and rules (no framework code)
│       ├── application/  # Use cases and ports (interfaces)
│       └── adapters/
│           └── inbound/
│               └── http/ # Axum routes (driving adapter)
└── frontend/             # Leptos SSR + hydration app
    ├── Cargo.toml        # cargo-leptos configuration lives here
    ├── Containerfile     # Builds the app with cargo-leptos
    ├── style/tailwind.css
    └── src/
        ├── main.rs       # SSR server (feature `ssr`)
        ├── lib.rs        # Hydration entry point (feature `hydrate`)
        └── app.rs        # HTML shell and root component
```

## Services

| Service    | Technology        | Host URL / port              | Notes                                   |
|------------|-------------------|------------------------------|-----------------------------------------|
| backend    | Axum 0.8, Tokio   | http://localhost:3000        | `GET /health` returns `ok`              |
| frontend   | Leptos 0.8 (SSR)  | http://localhost:3001        | Page is server-rendered and hydrated    |
| postgres   | PostgreSQL 17     | localhost:5433 (container 5432) | User/password/db `congreso` by default |

Host port `5433` is used for Postgres because a PostgreSQL server is often already running on the host at `5432`. Services inside the compose network talk to each other by service name (for example `postgres:5432`).

## Prerequisites

- **Rust** (stable toolchain, edition 2024 support) with the `wasm32-unknown-unknown` target:
  ```sh
  rustup target add wasm32-unknown-unknown
  ```
- **cargo-leptos** (frontend build tool). `cargo install cargo-leptos` may fail on older toolchains because of its dependency MSRV; the prebuilt binary works:
  ```sh
  curl -sL https://github.com/leptos-rs/cargo-leptos/releases/download/v0.3.11/cargo-leptos-aarch64-apple-darwin.tar.gz | tar -xz
  cp cargo-leptos-aarch64-apple-darwin/cargo-leptos ~/.cargo/bin/
  ```
  Use the `x86_64-apple-darwin` or `*-unknown-linux-gnu` asset for other platforms.
- **Podman** and **podman compose** (macOS):
  ```sh
  brew install podman podman-compose
  podman machine init
  podman machine set --memory 8192 --cpus 6
  podman machine start
  ```
  The 8 GB / 6 CPU setting matters: the frontend release build was killed by the OOM killer with the default 2 GB VM.

## Running with Podman (recommended)

From the repository root:

```sh
podman compose up --build -d   # build images and start all services
podman compose ps              # check status
podman compose logs -f         # follow logs
podman compose down            # stop and remove containers (keeps the database volume)
```

Then open:

- Frontend: http://localhost:3001
- Backend health check: http://localhost:3000/health

Database credentials can be overridden with environment variables `POSTGRES_USER`, `POSTGRES_PASSWORD` and `POSTGRES_DB` (defaults: `congreso`). Data is stored in the `postgres_data` volume. To wipe it, run `podman compose down -v`.

## Running locally without containers

Backend:

```sh
cd backend
cargo run                       # http://localhost:3000
```

Frontend (development server with hot reload on port 3002):

```sh
cd frontend
cargo leptos watch              # http://127.0.0.1:3001
```

Production-style build and run:

```sh
cd frontend
cargo leptos build --release
```

## Testing and checks

```sh
cd backend && cargo check && cargo test
cd frontend && cargo leptos build
```

There are no automated tests yet.

## Configuration

| Component | Setting                 | Where                                       |
|-----------|-------------------------|---------------------------------------------|
| backend   | Bind address `0.0.0.0:3000` | `backend/src/main.rs`                   |
| frontend  | Site address `127.0.0.1:3001`, reload port `3002` | `frontend/Cargo.toml` (`[package.metadata.leptos]`) |
| frontend (container) | `LEPTOS_*` env vars (`LEPTOS_SITE_ADDR=0.0.0.0:3001`, etc.) | `frontend/Containerfile` |
| postgres  | `POSTGRES_USER`, `POSTGRES_PASSWORD`, `POSTGRES_DB` | `compose.yaml` (env vars with defaults) |

The backend does not yet read a database URL or any other configuration.

## Troubleshooting

- **`bind: address already in use` on port 5432**: a local PostgreSQL is running. The compose file already maps the container to `5433`; stop the local server or change the mapping if you prefer.
- **Build killed (`SIGKILL`) during frontend image build**: increase Podman VM memory (`podman machine set --memory 8192`), restart the machine, then rebuild.
- **`unable to retrieve auth token ... unauthorized` when pulling images**: intermittent Docker Hub rate limiting or auth failures. Retry, or pull the base image manually with `podman pull docker.io/library/<image>`.
- **`Permission denied (publickey)` on push**: the SSH key is not registered for the GitHub account that owns the repository.

## Roadmap

- Add a PostgreSQL adapter and database migrations in the backend.
- Define domain entities and ports for the government data sources.
- Implement adapters that call Colombia's public APIs.
- Build the frontend pages on top of the backend API.
