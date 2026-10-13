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
