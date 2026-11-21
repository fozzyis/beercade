-- B.E.E.R.C.A.D.E. — Cloudflare D1 schema, migration 0003
-- Stripe webhook event idempotency ledger, session codes, and unique session indexes.

CREATE TABLE IF NOT EXISTS stripe_webhook_events (
    event_id TEXT PRIMARY KEY,
    event_type TEXT NOT NULL,
    stripe_session_id TEXT,
    outcome TEXT NOT NULL,
    processed_at_ms INTEGER NOT NULL
);

ALTER TABLE token_vouchers ADD COLUMN session_code TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_token_vouchers_stripe_session ON token_vouchers(stripe_session_id) WHERE stripe_session_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_token_vouchers_session_code ON token_vouchers(session_code) WHERE session_code IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_function_bookings_stripe_session ON function_bookings(stripe_session_id) WHERE stripe_session_id IS NOT NULL;
