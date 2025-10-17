-- Initial BeemFlow schema
-- Compatible with both SQLite and PostgreSQL

-- Runs table (execution tracking)
CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY,
    flow_name TEXT,
    event TEXT,
    vars TEXT,
    status TEXT,
    started_at BIGINT,
    ended_at BIGINT
);

-- Steps table (step execution tracking)
CREATE TABLE IF NOT EXISTS steps (
    id TEXT PRIMARY KEY,
    run_id TEXT,
    step_name TEXT,
    status TEXT,
    started_at BIGINT,
    ended_at BIGINT,
    outputs TEXT,
    error TEXT
);

-- Waits table (timeout/wait tracking)
CREATE TABLE IF NOT EXISTS waits (
    token TEXT PRIMARY KEY,
    wake_at BIGINT
);

-- Paused runs table (await_event support)
CREATE TABLE IF NOT EXISTS paused_runs (
    token TEXT PRIMARY KEY,
    data TEXT
);

-- Flows table (flow definitions)
CREATE TABLE IF NOT EXISTS flows (
    name TEXT PRIMARY KEY,
    content TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

-- Flow versions table (deployment history)
CREATE TABLE IF NOT EXISTS flow_versions (
    flow_name TEXT NOT NULL,
    version TEXT NOT NULL,
    content TEXT NOT NULL,
    deployed_at BIGINT NOT NULL,
    PRIMARY KEY (flow_name, version)
);

-- Deployed flows table (current live versions)
CREATE TABLE IF NOT EXISTS deployed_flows (
    flow_name TEXT PRIMARY KEY,
    deployed_version TEXT NOT NULL,
    deployed_at BIGINT NOT NULL
);

-- Index for version queries
CREATE INDEX IF NOT EXISTS idx_flow_versions_name ON flow_versions(flow_name, deployed_at DESC);

-- OAuth credentials table
CREATE TABLE IF NOT EXISTS oauth_credentials (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    integration TEXT NOT NULL,
    access_token TEXT NOT NULL,
    refresh_token TEXT,
    expires_at BIGINT,
    scope TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    UNIQUE(provider, integration)
);

-- OAuth providers table
CREATE TABLE IF NOT EXISTS oauth_providers (
    id TEXT PRIMARY KEY,
    client_id TEXT NOT NULL,
    client_secret TEXT NOT NULL,
    auth_url TEXT NOT NULL,
    token_url TEXT NOT NULL,
    scopes TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

-- OAuth clients table (for BeemFlow as OAuth server)
CREATE TABLE IF NOT EXISTS oauth_clients (
    id TEXT PRIMARY KEY,
    secret TEXT NOT NULL,
    name TEXT NOT NULL,
    redirect_uris TEXT NOT NULL,
    grant_types TEXT NOT NULL,
    response_types TEXT NOT NULL,
    scope TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

-- OAuth tokens table (for BeemFlow as OAuth server)
CREATE TABLE IF NOT EXISTS oauth_tokens (
    id TEXT PRIMARY KEY,
    client_id TEXT NOT NULL,
    user_id TEXT,
    redirect_uri TEXT,
    scope TEXT,
    code TEXT UNIQUE,
    code_create_at BIGINT,
    code_expires_in BIGINT,
    code_challenge TEXT,
    code_challenge_method TEXT,
    access TEXT UNIQUE,
    access_create_at BIGINT,
    access_expires_in BIGINT,
    refresh TEXT UNIQUE,
    refresh_create_at BIGINT,
    refresh_expires_in BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

-- Performance indexes for runs queries
-- Composite index for flow_name + status + started_at queries (optimizes list_runs_by_flow_and_status)
CREATE INDEX IF NOT EXISTS idx_runs_flow_status_time ON runs(flow_name, status, started_at DESC);

-- Index for steps by run_id (frequently queried for step outputs)
CREATE INDEX IF NOT EXISTS idx_steps_run_id ON steps(run_id);

-- Index for general time-based queries (list_runs with ORDER BY)
CREATE INDEX IF NOT EXISTS idx_runs_started_at ON runs(started_at DESC);
