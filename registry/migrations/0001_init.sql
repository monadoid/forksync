CREATE TABLE IF NOT EXISTS sources (
    id TEXT PRIMARY KEY,
    upstream_repo TEXT NOT NULL,
    source_repo TEXT NOT NULL,
    tracked_branch TEXT NOT NULL,
    display_name TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    visibility TEXT NOT NULL DEFAULT 'public',
    stars INTEGER NOT NULL DEFAULT 0,
    forks INTEGER NOT NULL DEFAULT 0,
    last_pushed_at TEXT,
    verified INTEGER NOT NULL DEFAULT 0,
    verified_by_login TEXT,
    published_by_login TEXT NOT NULL,
    published_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_sources_public_verified
    ON sources(visibility, verified, updated_at);

CREATE INDEX IF NOT EXISTS idx_sources_upstream
    ON sources(upstream_repo);

CREATE INDEX IF NOT EXISTS idx_sources_source_repo
    ON sources(source_repo);
