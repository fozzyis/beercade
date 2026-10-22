-- B.E.E.R.C.A.D.E. — Cloudflare D1 schema, migration 0001
-- Historical machine analytics, high scores, token transactions and error
-- logs ingested from B.O.S.S. via /api/v1/edge/ingest.

-- ---------------------------------------------------------------------------
-- machine_analytics: one row per node telemetry heartbeat batched from BOSS.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS machine_analytics (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    venue_id           TEXT    NOT NULL,
    node_id            TEXT    NOT NULL,
    hardware           TEXT,
    soc                TEXT,
    cpu_temp_c         REAL,
    gpu_temp_c         REAL,
    npu_load_pct       REAL,
    active_core        TEXT,
    active_rom         TEXT,
    frame_time_ms      REAL,
    fps                REAL,
    -- 1000Hz evdev poll jitter stats (microseconds over the 1ms deadline)
    jitter_poll_hz     REAL,
    jitter_p50_us      REAL,
    jitter_p95_us      REAL,
    jitter_p99_us      REAL,
    jitter_max_us      REAL,
    jitter_dropped     INTEGER,
    -- P2P netplay (NULL when no session active)
    netplay_peer_id    TEXT,
    netplay_rtt_ms     REAL,
    netplay_jitter_ms  REAL,
    netplay_loss_pct   REAL,
    netplay_rollback   INTEGER,
    -- Credit counters snapshot
    credits_available  INTEGER,
    credits_total      INTEGER,
    credits_today      INTEGER,
    captured_at_ms     INTEGER NOT NULL,
    ingested_at_ms     INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_analytics_node_time
    ON machine_analytics (node_id, captured_at_ms);
CREATE INDEX IF NOT EXISTS idx_analytics_venue_time
    ON machine_analytics (venue_id, captured_at_ms);
CREATE INDEX IF NOT EXISTS idx_analytics_rom
    ON machine_analytics (active_rom, captured_at_ms);

-- ---------------------------------------------------------------------------
-- high_scores: score submissions from cabinets. Natural key prevents dupes
-- when BOSS retries a rejected batch.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS high_scores (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id           TEXT    NOT NULL,
    active_core       TEXT    NOT NULL,
    active_rom        TEXT    NOT NULL,
    game_name         TEXT    NOT NULL,
    score             INTEGER NOT NULL,
    initials          TEXT    NOT NULL,
    achieved_at_ms    INTEGER NOT NULL,
    ingested_at_ms    INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_scores_natural_key
    ON high_scores (active_rom, initials, score, achieved_at_ms);
CREATE INDEX IF NOT EXISTS idx_scores_rom_score
    ON high_scores (active_rom, score DESC);

-- ---------------------------------------------------------------------------
-- token_transactions: credit add/dispense/refund ledger.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS token_transactions (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    venue_id          TEXT    NOT NULL,
    node_id           TEXT    NOT NULL,
    kind              TEXT    NOT NULL CHECK (kind IN ('add', 'dispense', 'refund')),
    credits           INTEGER NOT NULL,
    balance_after     INTEGER,
    command_id        TEXT,
    created_at_ms     INTEGER NOT NULL,
    ingested_at_ms    INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tx_node_time
    ON token_transactions (node_id, created_at_ms);
CREATE INDEX IF NOT EXISTS idx_tx_venue_time
    ON token_transactions (venue_id, created_at_ms);

-- ---------------------------------------------------------------------------
-- error_logs: structured node error reports.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS error_logs (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    venue_id          TEXT    NOT NULL,
    node_id           TEXT    NOT NULL,
    level             TEXT    NOT NULL CHECK (level IN ('warn', 'error', 'fatal')),
    source            TEXT,
    message           TEXT    NOT NULL,
    detail            TEXT,   -- JSON blob
    occurred_at_ms    INTEGER NOT NULL,
    ingested_at_ms    INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_errors_node_time
    ON error_logs (node_id, occurred_at_ms);
CREATE INDEX IF NOT EXISTS idx_errors_level_time
    ON error_logs (level, occurred_at_ms);
