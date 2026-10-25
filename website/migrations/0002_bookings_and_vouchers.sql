-- B.E.E.R.C.A.D.E. — Cloudflare D1 schema, migration 0002
-- Online function bookings and Stripe/QR token pack vouchers.

-- ---------------------------------------------------------------------------
-- function_bookings: group/event reservations created via /bookings
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS function_bookings (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    booking_ref         TEXT    NOT NULL UNIQUE,
    event_type          TEXT    NOT NULL,
    booking_date        TEXT    NOT NULL,
    booking_time        TEXT    NOT NULL,
    headcount           INTEGER NOT NULL,
    area                TEXT    NOT NULL,
    drink_bundle        TEXT,
    food_preorders_json TEXT,
    deposit_amount      INTEGER NOT NULL,
    food_total          INTEGER NOT NULL DEFAULT 0,
    token_addon_amount  INTEGER NOT NULL DEFAULT 0,
    total_amount        INTEGER NOT NULL,
    contact_name        TEXT    NOT NULL,
    contact_email       TEXT    NOT NULL,
    contact_phone       TEXT    NOT NULL,
    stripe_session_id   TEXT,
    status              TEXT    NOT NULL DEFAULT 'CONFIRMED',
    created_at_ms       INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_bookings_date_area
    ON function_bookings (booking_date, area);
CREATE INDEX IF NOT EXISTS idx_bookings_email
    ON function_bookings (contact_email, created_at_ms DESC);

-- ---------------------------------------------------------------------------
-- token_vouchers: online token packs purchased via /tokens
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS token_vouchers (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    voucher_id          TEXT    NOT NULL UNIQUE,
    email               TEXT    NOT NULL,
    amount_aud          INTEGER NOT NULL,
    base_tokens         INTEGER NOT NULL,
    bonus_tokens        INTEGER NOT NULL DEFAULT 0,
    total_tokens        INTEGER NOT NULL,
    target_node_id      TEXT,
    stripe_session_id   TEXT,
    status              TEXT    NOT NULL DEFAULT 'ISSUED',
    redeemed_at_ms      INTEGER,
    created_at_ms       INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_vouchers_email
    ON token_vouchers (email, created_at_ms DESC);
CREATE INDEX IF NOT EXISTS idx_vouchers_status
    ON token_vouchers (status, created_at_ms DESC);
