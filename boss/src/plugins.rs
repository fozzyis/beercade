//! B.O.S.S. Hybrid Event-Hook + UI Panel Plugin Engine.
//!
//! Combines compiled Rust first-party plugins (`BossPlugin` trait) with
//! declarative JSON manifests and external webhook subscribers, all hooked
//! into the central B.O.S.S. event bus and exposed to `boss/kiosk_ui`.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::models::{
    CabinetCommand, CommandRecord, ErrorEvent, HighScoreEntry, NodeTelemetry, TokenTransaction,
    now_epoch_ms,
};
use crate::state::{EdgeItem, SharedState, SseChannel};

// ---------------------------------------------------------------------------
// Core Plugin Manifest & Event Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginCategory {
    PosPayments,
    VenueLighting,
    TournamentEngine,
    RfidPeripherals,
    AlertingSinks,
    WebhookCustom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginUiPanelSpec {
    pub tab_label: String,
    pub icon: String,
    pub summary_metric_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub category: PluginCategory,
    pub description: String,
    pub author: String,
    pub enabled: bool,
    pub builtin: bool,
    pub subscribed_events: Vec<String>,
    pub capabilities: Vec<String>,
    pub ui_panel: PluginUiPanelSpec,
    #[serde(default)]
    pub webhook_url: Option<String>,
}

/// Strongly-typed operational events fanned out across the plugin registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum PluginEvent {
    TelemetryHeartbeat(NodeTelemetry),
    NodeOnline {
        node_id: String,
        at_epoch_ms: u64,
    },
    NodeOffline {
        node_ids: Vec<String>,
        at_epoch_ms: u64,
    },
    ErrorReported(ErrorEvent),
    HighScore(HighScoreEntry),
    CreditTransaction(TokenTransaction),
    CommandUpdated(CommandRecord),
}

impl PluginEvent {
    pub fn event_name(&self) -> &'static str {
        match self {
            PluginEvent::TelemetryHeartbeat(_) => "telemetry_heartbeat",
            PluginEvent::NodeOnline { .. } => "node_online",
            PluginEvent::NodeOffline { .. } => "node_offline",
            PluginEvent::ErrorReported(_) => "error_reported",
            PluginEvent::HighScore(_) => "high_score",
            PluginEvent::CreditTransaction(_) => "credit_transaction",
            PluginEvent::CommandUpdated(_) => "command_updated",
        }
    }
}

/// Snapshot returned by `GET /api/v1/plugins` for each registered plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDescriptor {
    pub manifest: PluginManifest,
    pub state: Value,
    pub updated_at_epoch_ms: u64,
}

#[axum::async_trait]
pub trait BossPlugin: Send + Sync {
    fn id(&self) -> &str;
    async fn manifest(&self) -> PluginManifest;
    async fn set_enabled(&self, enabled: bool);
    async fn is_enabled(&self) -> bool;
    async fn on_event(&self, event: &PluginEvent, app_state: &SharedState);
    async fn invoke_action(
        &self,
        action: &str,
        payload: Value,
        app_state: &SharedState,
    ) -> Result<Value, String>;
    async fn state_snapshot(&self) -> Value;
}

// ---------------------------------------------------------------------------
// 1. POS & Payment Terminals Plugin (`pos-payments` — Square + Stripe + Nayax)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SquareIntegrationConfig {
    pub enabled: bool,
    pub environment: String, // "sandbox" | "production"
    pub location_id: String,
    pub terminal_device_id: String,
    pub access_token_configured: bool,
    pub webhook_signature_verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentTerminal {
    pub id: String,
    pub provider: String,
    pub label: String,
    pub location: String,
    pub status: String,
    pub last_ping_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditPackTier {
    pub id: String,
    pub label: String,
    pub credits: u32,
    pub price_cents: u32,
    pub badge: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AstroGameTimeTier {
    pub id: String,
    pub label: String,
    pub duration_mins: u32,
    pub included_credits: u32,
    pub price_cents: u32,
    pub badge: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveTimedSession {
    pub session_id: String,
    pub pass_code: String,
    pub patron_name: String,
    pub node_id: String,
    pub duration_mins: u32,
    pub started_at_ms: u64,
    pub expires_at_ms: u64,
    pub provider: String,
    pub status: String, // "active" | "completed"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrCheckoutSession {
    pub session_id: String,
    pub node_id: String,
    pub pack_id: String,
    pub credits: u32,
    pub duration_mins: Option<u32>,
    pub amount_cents: u32,
    pub provider: String,
    pub pass_code: Option<String>,
    pub status: String,
    pub qr_payload: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PosLedgerRecord {
    pub id: String,
    pub provider: String,
    pub node_id: String,
    pub credits: u32,
    pub amount_cents: u32,
    pub reference: String,
    pub created_at_ms: u64,
}

struct PosState {
    enabled: bool,
    square_config: SquareIntegrationConfig,
    terminals: Vec<PaymentTerminal>,
    tiers: Vec<CreditPackTier>,
    game_time_tiers: Vec<AstroGameTimeTier>,
    active_timed_sessions: VecDeque<ActiveTimedSession>,
    active_sessions: VecDeque<QrCheckoutSession>,
    ledger: VecDeque<PosLedgerRecord>,
    total_revenue_cents: u64,
    total_credits_sold: u64,
    transactions_count: u64,
}

pub struct PosPaymentsPlugin {
    inner: RwLock<PosState>,
}

impl PosPaymentsPlugin {
    pub fn new() -> Self {
        let now = now_epoch_ms();
        let has_sq_token = std::env::var("SQUARE_ACCESS_TOKEN")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        let sq_env = std::env::var("SQUARE_ENVIRONMENT").unwrap_or_else(|_| "production".into());
        let sq_loc = std::env::var("SQUARE_LOCATION_ID").unwrap_or_else(|_| "L_BEERCADE_REDFERN".into());
        let sq_dev = std::env::var("SQUARE_TERMINAL_DEVICE_ID").unwrap_or_else(|_| "sq_term_kiosk_01".into());

        Self {
            inner: RwLock::new(PosState {
                enabled: true,
                square_config: SquareIntegrationConfig {
                    enabled: true,
                    environment: sq_env,
                    location_id: sq_loc,
                    terminal_device_id: sq_dev,
                    access_token_configured: has_sq_token,
                    webhook_signature_verified: true,
                },
                terminals: vec![
                    PaymentTerminal {
                        id: "term-square-kiosk-01".into(),
                        provider: "Square Terminal (NFC / Chip / QR)".into(),
                        label: "B.O.S.S. Main Kiosk Reader".into(),
                        location: "Front Entrance Supervisor Kiosk".into(),
                        status: "online".into(),
                        last_ping_ms: now,
                    },
                    PaymentTerminal {
                        id: "term-square-bar-02".into(),
                        provider: "Square Register POS & KDS".into(),
                        label: "Taproom Bar & Kitchen POS".into(),
                        location: "Main Bar Tap Wall".into(),
                        status: "online".into(),
                        last_ping_ms: now,
                    },
                    PaymentTerminal {
                        id: "term-stripe-01".into(),
                        provider: "Stripe Terminal WisePOS E".into(),
                        label: "Secondary Kiosk Reader".into(),
                        location: "Arcade Floor North".into(),
                        status: "online".into(),
                        last_ping_ms: now,
                    },
                    PaymentTerminal {
                        id: "term-nayax-01".into(),
                        provider: "Nayax VPOS Touch".into(),
                        label: "Coin-Pulse Bridge".into(),
                        location: "VIP Booth Lounge".into(),
                        status: "online".into(),
                        last_ping_ms: now,
                    },
                ],
                tiers: vec![
                    CreditPackTier {
                        id: "pack-1".into(),
                        label: "Single Drop".into(),
                        credits: 1,
                        price_cents: 100,
                        badge: None,
                    },
                    CreditPackTier {
                        id: "pack-5".into(),
                        label: "Squad 5-Pack".into(),
                        credits: 5,
                        price_cents: 450,
                        badge: Some("POPULAR".into()),
                    },
                    CreditPackTier {
                        id: "pack-15".into(),
                        label: "Arcade Boss 15-Pack".into(),
                        credits: 15,
                        price_cents: 1200,
                        badge: Some("BEST VALUE".into()),
                    },
                ],
                game_time_tiers: vec![
                    AstroGameTimeTier {
                        id: "astro-30m".into(),
                        label: "30-Min A.S.T.R.O. Session".into(),
                        duration_mins: 30,
                        included_credits: 10,
                        price_cents: 1200,
                        badge: Some("QUICK PLAY".into()),
                    },
                    AstroGameTimeTier {
                        id: "astro-60m".into(),
                        label: "60-Min Unlimited A.S.T.R.O. Pass".into(),
                        duration_mins: 60,
                        included_credits: 25,
                        price_cents: 2000,
                        badge: Some("MOST POPULAR".into()),
                    },
                    AstroGameTimeTier {
                        id: "astro-120m".into(),
                        label: "2-Hour A.S.T.R.O. Marathon Pass".into(),
                        duration_mins: 120,
                        included_credits: 60,
                        price_cents: 3500,
                        badge: Some("BEST VALUE".into()),
                    },
                ],
                active_timed_sessions: VecDeque::from([ActiveTimedSession {
                    session_id: "sess_astro_01".into(),
                    pass_code: "ASTRO-SQ-8841".into(),
                    patron_name: "Alex (Online Square Pass)".into(),
                    node_id: "ASTRO-01".into(),
                    duration_mins: 60,
                    started_at_ms: now.saturating_sub(18 * 60 * 1000),
                    expires_at_ms: now + (42 * 60 * 1000),
                    provider: "Square Online Checkout".into(),
                    status: "active".into(),
                }]),
                active_sessions: VecDeque::new(),
                ledger: VecDeque::new(),
                total_revenue_cents: 0,
                total_credits_sold: 0,
                transactions_count: 0,
            }),
        }
    }
}

#[axum::async_trait]
impl BossPlugin for PosPaymentsPlugin {
    fn id(&self) -> &str {
        "pos-payments"
    }

    async fn manifest(&self) -> PluginManifest {
        let enabled = self.inner.read().await.enabled;
        PluginManifest {
            id: "pos-payments".into(),
            name: "POS & Payment Terminals (Square / Stripe / Nayax)".into(),
            version: "1.2.0".into(),
            category: PluginCategory::PosPayments,
            description: "Square Terminal & Online Checkout, A.S.T.R.O. timed game-session passes, Stripe WisePOS, and Nayax coin-pulse bridge.".into(),
            author: "B.E.E.R.C.A.D.E. Core".into(),
            enabled,
            builtin: true,
            subscribed_events: vec!["credit_transaction".into()],
            capabilities: vec![
                "square_terminal".into(),
                "square_online_checkout".into(),
                "astro_timed_sessions".into(),
                "qr_checkout".into(),
                "terminal_tap".into(),
                "daily_reconciliation".into(),
            ],
            ui_panel: PluginUiPanelSpec {
                tab_label: "POS & Square".into(),
                icon: "credit-card".into(),
                summary_metric_key: "total_credits_sold".into(),
            },
            webhook_url: None,
        }
    }

    async fn set_enabled(&self, enabled: bool) {
        self.inner.write().await.enabled = enabled;
    }

    async fn is_enabled(&self) -> bool {
        self.inner.read().await.enabled
    }

    async fn on_event(&self, event: &PluginEvent, _app_state: &SharedState) {
        if let PluginEvent::CreditTransaction(tx) = event {
            if tx.kind == "add" {
                let mut st = self.inner.write().await;
                if let Some(term) = st.terminals.first_mut() {
                    term.last_ping_ms = now_epoch_ms();
                }
            }
        }
    }

    async fn invoke_action(
        &self,
        action: &str,
        payload: Value,
        app_state: &SharedState,
    ) -> Result<Value, String> {
        match action {
            "create_qr_session" | "create_square_checkout" => {
                let node_id = payload
                    .get("node_id")
                    .and_then(Value::as_str)
                    .unwrap_or("ASTRO-01")
                    .trim()
                    .to_string();
                let pack_id = payload
                    .get("pack_id")
                    .and_then(Value::as_str)
                    .unwrap_or("pack-5");
                let provider = payload
                    .get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or("Square Online Checkout")
                    .to_string();

                let mut st = self.inner.write().await;

                // Check if pack_id is an A.S.T.R.O. timed game-time tier first
                if let Some(gt) = st.game_time_tiers.iter().find(|t| t.id == pack_id).cloned() {
                    let session_id = format!("sq_gt_{}", &uuid::Uuid::new_v4().to_string()[..8]);
                    let pass_code = format!(
                        "ASTRO-SQ-{}",
                        &uuid::Uuid::new_v4().to_string()[..4].to_uppercase()
                    );
                    let qr_payload = format!(
                        "https://squareup.com/checkout/beercade?session={}&node={}&pass={}&mins={}",
                        session_id, node_id, pass_code, gt.duration_mins
                    );
                    let session = QrCheckoutSession {
                        session_id: session_id.clone(),
                        node_id,
                        pack_id: gt.id,
                        credits: gt.included_credits,
                        duration_mins: Some(gt.duration_mins),
                        amount_cents: gt.price_cents,
                        provider,
                        pass_code: Some(pass_code),
                        status: "pending_scan".into(),
                        qr_payload,
                        created_at_ms: now_epoch_ms(),
                    };
                    if st.active_sessions.len() >= 25 {
                        st.active_sessions.pop_back();
                    }
                    st.active_sessions.push_front(session.clone());
                    return Ok(serde_json::to_value(session).unwrap_or_default());
                }

                let tier = st
                    .tiers
                    .iter()
                    .find(|t| t.id == pack_id)
                    .cloned()
                    .unwrap_or(CreditPackTier {
                        id: "pack-1".into(),
                        label: "Single Drop".into(),
                        credits: 1,
                        price_cents: 100,
                        badge: None,
                    });

                let session_id = format!("sq_{}", &uuid::Uuid::new_v4().to_string()[..8]);
                let qr_payload = format!(
                    "https://squareup.com/checkout/beercade?session={}&node={}&credits={}&loc={}",
                    session_id, node_id, tier.credits, st.square_config.location_id
                );
                let session = QrCheckoutSession {
                    session_id: session_id.clone(),
                    node_id,
                    pack_id: tier.id,
                    credits: tier.credits,
                    duration_mins: None,
                    amount_cents: tier.price_cents,
                    provider,
                    pass_code: None,
                    status: "pending_scan".into(),
                    qr_payload,
                    created_at_ms: now_epoch_ms(),
                };
                if st.active_sessions.len() >= 25 {
                    st.active_sessions.pop_back();
                }
                st.active_sessions.push_front(session.clone());
                Ok(serde_json::to_value(session).unwrap_or_default())
            }
            "activate_game_time_pass" => {
                let node_id = payload
                    .get("node_id")
                    .and_then(Value::as_str)
                    .unwrap_or("ASTRO-01")
                    .trim()
                    .to_string();
                let duration_mins = payload
                    .get("duration_mins")
                    .and_then(Value::as_u64)
                    .unwrap_or(60)
                    .clamp(15, 240) as u32;
                let credits = payload
                    .get("credits")
                    .and_then(Value::as_u64)
                    .unwrap_or((duration_mins / 2) as u64)
                    .clamp(1, 100) as u32;
                let amount_cents = payload
                    .get("amount_cents")
                    .and_then(Value::as_u64)
                    .unwrap_or(2000) as u32;
                let patron_name = payload
                    .get("patron_name")
                    .and_then(Value::as_str)
                    .unwrap_or("Square Online Player")
                    .to_string();
                let pass_code = payload
                    .get("pass_code")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        format!(
                            "ASTRO-SQ-{}",
                            &uuid::Uuid::new_v4().to_string()[..4].to_uppercase()
                        )
                    });
                let provider = payload
                    .get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or("Square Online Game-Time Pass")
                    .to_string();

                let now = now_epoch_ms();
                let duration_secs = duration_mins * 60;
                let expires_at_ms = now + (duration_mins as u64) * 60 * 1000;
                let session_id = format!("sess_{}", &uuid::Uuid::new_v4().to_string()[..8]);

                // Enqueue session countdown to put the station in Managed mode
                app_state
                    .enqueue_command(
                        &node_id,
                        CabinetCommand::StartSession {
                            session_id: session_id.clone(),
                            duration_secs,
                        },
                    )
                    .await;

                // Dispatch credits to the A.S.T.R.O. cabinet node immediately
                let cmd_record = app_state
                    .enqueue_command(&node_id, CabinetCommand::AddCredits { credits })
                    .await;

                let timed_session = ActiveTimedSession {
                    session_id,
                    pass_code: pass_code.clone(),
                    patron_name: patron_name.clone(),
                    node_id: node_id.clone(),
                    duration_mins,
                    started_at_ms: now,
                    expires_at_ms,
                    provider: provider.clone(),
                    status: "active".into(),
                };

                let tx = TokenTransaction {
                    node_id: node_id.clone(),
                    kind: "square_game_time_pass".into(),
                    credits,
                    balance_after: credits,
                    command_id: Some(cmd_record.id.clone()),
                    created_at_epoch_ms: now,
                };
                app_state.record_transaction(tx.clone()).await;
                app_state.buffer_edge_item(EdgeItem::Transaction(tx)).await;

                let mut st = self.inner.write().await;
                if st.active_timed_sessions.len() >= 25 {
                    st.active_timed_sessions.pop_back();
                }
                st.active_timed_sessions.push_front(timed_session.clone());
                st.total_revenue_cents += amount_cents as u64;
                st.total_credits_sold += credits as u64;
                st.transactions_count += 1;

                let entry = PosLedgerRecord {
                    id: pass_code.clone(),
                    provider: format!("{provider} ({duration_mins}m · {patron_name})"),
                    node_id: node_id.clone(),
                    credits,
                    amount_cents,
                    reference: cmd_record.id.clone(),
                    created_at_ms: now,
                };
                if st.ledger.len() >= 50 {
                    st.ledger.pop_back();
                }
                st.ledger.push_front(entry.clone());

                Ok(json!({
                    "ok": true,
                    "timed_session": timed_session,
                    "ledger_entry": entry,
                    "command_id": cmd_record.id,
                }))
            }
            "configure_square" => {
                let mut st = self.inner.write().await;
                if let Some(env) = payload.get("environment").and_then(Value::as_str) {
                    st.square_config.environment = env.to_string();
                }
                if let Some(loc) = payload.get("location_id").and_then(Value::as_str) {
                    st.square_config.location_id = loc.to_string();
                }
                if let Some(dev) = payload.get("terminal_device_id").and_then(Value::as_str) {
                    st.square_config.terminal_device_id = dev.to_string();
                }
                Ok(json!({
                    "ok": true,
                    "square_config": st.square_config
                }))
            }
            "simulate_tap_pay" | "complete_payment" => {
                let node_id = payload
                    .get("node_id")
                    .and_then(Value::as_str)
                    .unwrap_or("ASTRO-01")
                    .trim()
                    .to_string();
                let credits = payload
                    .get("credits")
                    .and_then(Value::as_u64)
                    .unwrap_or(5)
                    .clamp(1, 100) as u32;
                let amount_cents = payload
                    .get("amount_cents")
                    .and_then(Value::as_u64)
                    .unwrap_or((credits * 100) as u64) as u32;
                let provider = payload
                    .get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or("Square Terminal NFC")
                    .to_string();
                let session_id_opt = payload
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);

                let cmd_record = app_state
                    .enqueue_command(&node_id, CabinetCommand::AddCredits { credits })
                    .await;

                let balance_after = {
                    let nodes = app_state.nodes.read().await;
                    nodes
                        .get(&node_id)
                        .map(|(t, _)| t.credits.credits_available.saturating_add(credits))
                        .unwrap_or(credits)
                };

                let tx = TokenTransaction {
                    node_id: node_id.clone(),
                    kind: "pos_purchase".into(),
                    credits,
                    balance_after,
                    command_id: Some(cmd_record.id.clone()),
                    created_at_epoch_ms: now_epoch_ms(),
                };
                app_state.record_transaction(tx.clone()).await;
                app_state.buffer_edge_item(EdgeItem::Transaction(tx)).await;

                let mut st = self.inner.write().await;
                if let Some(sid) = &session_id_opt {
                    for s in st.active_sessions.iter_mut() {
                        if &s.session_id == sid {
                            s.status = "completed".into();
                        }
                    }
                }
                st.total_revenue_cents += amount_cents as u64;
                st.total_credits_sold += credits as u64;
                st.transactions_count += 1;

                let entry = PosLedgerRecord {
                    id: format!("pos_{}", &uuid::Uuid::new_v4().to_string()[..8]),
                    provider,
                    node_id: node_id.clone(),
                    credits,
                    amount_cents,
                    reference: cmd_record.id.clone(),
                    created_at_ms: now_epoch_ms(),
                };
                if st.ledger.len() >= 50 {
                    st.ledger.pop_back();
                }
                st.ledger.push_front(entry.clone());

                Ok(json!({
                    "ok": true,
                    "ledger_entry": entry,
                    "command_id": cmd_record.id,
                    "projected_balance": balance_after
                }))
            }
            "reset_daily_totals" => {
                let mut st = self.inner.write().await;
                st.total_revenue_cents = 0;
                st.total_credits_sold = 0;
                st.transactions_count = 0;
                st.ledger.clear();
                Ok(json!({ "ok": true }))
            }
            "book_timeslot" => {
                let node_id = payload
                    .get("node_id")
                    .and_then(Value::as_str)
                    .unwrap_or("ASTRO-01")
                    .trim()
                    .to_string();
                let duration_mins = payload
                    .get("duration_mins")
                    .and_then(Value::as_u64)
                    .unwrap_or(60)
                    .clamp(15, 240) as u32;
                let patron_name = payload
                    .get("patron_name")
                    .and_then(Value::as_str)
                    .unwrap_or("Kiosk Patron")
                    .to_string();
                let amount_cents = (duration_mins / 30) * 1000;
                let booking_ref = format!("SLOT-{}", &uuid::Uuid::new_v4().to_string()[..6].to_uppercase());
                let now = now_epoch_ms();
                let duration_secs = duration_mins * 60;
                let session_id = format!("sess_{}", &uuid::Uuid::new_v4().to_string()[..6]);

                // Enqueue session countdown to put the station in Managed mode
                app_state
                    .enqueue_command(
                        &node_id,
                        CabinetCommand::StartSession {
                            session_id: session_id.clone(),
                            duration_secs,
                        },
                    )
                    .await;

                let mut st = self.inner.write().await;
                st.total_revenue_cents += amount_cents as u64;
                st.transactions_count += 1;
                let timed = ActiveTimedSession {
                    session_id,
                    pass_code: booking_ref.clone(),
                    patron_name: patron_name.clone(),
                    node_id: node_id.clone(),
                    duration_mins,
                    started_at_ms: now,
                    expires_at_ms: now + (duration_mins as u64) * 60 * 1000,
                    provider: "Square Kiosk Time-Slot".into(),
                    status: "active".into(),
                };
                if st.active_timed_sessions.len() >= 25 {
                    st.active_timed_sessions.pop_back();
                }
                st.active_timed_sessions.push_front(timed);

                let entry = PosLedgerRecord {
                    id: booking_ref.clone(),
                    provider: format!("Square Time-Slot ({duration_mins}m · {patron_name})"),
                    node_id: node_id.clone(),
                    credits: 0,
                    amount_cents,
                    reference: booking_ref.clone(),
                    created_at_ms: now,
                };
                if st.ledger.len() >= 50 {
                    st.ledger.pop_back();
                }
                st.ledger.push_front(entry);

                Ok(json!({
                    "ok": true,
                    "booking_ref": booking_ref,
                    "node_id": node_id,
                    "duration_mins": duration_mins,
                    "amount_cents": amount_cents,
                }))
            }
            "submit_fb_order" => {
                let station = payload
                    .get("station")
                    .and_then(Value::as_str)
                    .unwrap_or("ASTRO-01")
                    .to_string();
                let summary = payload
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or("Bar Order")
                    .to_string();
                let amount_cents = payload
                    .get("amount_cents")
                    .and_then(Value::as_u64)
                    .unwrap_or(2000) as u32;
                let order_ref = format!("SQ-ORD-{}", &uuid::Uuid::new_v4().to_string()[..6].to_uppercase());

                let mut st = self.inner.write().await;
                st.total_revenue_cents += amount_cents as u64;
                st.transactions_count += 1;
                let entry = PosLedgerRecord {
                    id: order_ref.clone(),
                    provider: format!("Square KDS Order ({summary})"),
                    node_id: station.clone(),
                    credits: 0,
                    amount_cents,
                    reference: order_ref.clone(),
                    created_at_ms: now_epoch_ms(),
                };
                if st.ledger.len() >= 50 {
                    st.ledger.pop_back();
                }
                st.ledger.push_front(entry);

                Ok(json!({
                    "ok": true,
                    "order_ref": order_ref,
                    "station": station,
                    "amount_cents": amount_cents,
                }))
            }
            _ => Err(format!("Unsupported action '{action}' for pos-payments")),
        }
    }

    async fn state_snapshot(&self) -> Value {
        let st = self.inner.read().await;
        json!({
            "square_config": st.square_config,
            "terminals": st.terminals,
            "tiers": st.tiers,
            "game_time_tiers": st.game_time_tiers,
            "active_timed_sessions": st.active_timed_sessions,
            "active_sessions": st.active_sessions,
            "ledger": st.ledger,
            "total_revenue_cents": st.total_revenue_cents,
            "total_credits_sold": st.total_credits_sold,
            "transactions_count": st.transactions_count,
        })
    }
}

// ---------------------------------------------------------------------------
// 2. Venue Lighting & Audio Plugin (`venue-lighting` — Multi-Zone / Multi-System)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightingZone {
    pub id: String,
    pub name: String,
    pub active_scene: String,
    pub brightness_pct: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightingFixture {
    pub id: String,
    pub name: String,
    pub zone: String,
    pub protocol: String,
    pub endpoint: String,
    pub universe: u16,
    pub active_scene: String,
    pub brightness_pct: u8,
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightingCueLog {
    pub scene: String,
    pub zone: String,
    pub reason: String,
    pub triggered_at_ms: u64,
}

struct LightingState {
    enabled: bool,
    active_scene: String,
    master_brightness_pct: u8,
    strobe_on_high_score: bool,
    pulse_on_credit_insert: bool,
    red_alert_on_node_offline: bool,
    zones: Vec<LightingZone>,
    fixtures: Vec<LightingFixture>,
    cue_history: VecDeque<LightingCueLog>,
}

pub struct VenueLightingPlugin {
    inner: RwLock<LightingState>,
}

impl VenueLightingPlugin {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(LightingState {
                enabled: true,
                active_scene: "arcade_attract".into(),
                master_brightness_pct: 85,
                strobe_on_high_score: true,
                pulse_on_credit_insert: true,
                red_alert_on_node_offline: true,
                zones: vec![
                    LightingZone {
                        id: "arcade_floor".into(),
                        name: "Main Arcade Floor & Cabinet Row".into(),
                        active_scene: "arcade_attract".into(),
                        brightness_pct: 90,
                    },
                    LightingZone {
                        id: "tournament_stage".into(),
                        name: "Tournament Stage & Broadcast Rig".into(),
                        active_scene: "cyberpunk_neon".into(),
                        brightness_pct: 100,
                    },
                    LightingZone {
                        id: "taproom_bar".into(),
                        name: "Taproom Bar & Craft Tap Wall".into(),
                        active_scene: "chill_lounge".into(),
                        brightness_pct: 75,
                    },
                    LightingZone {
                        id: "cabinet_marquees".into(),
                        name: "O.R.I.O.N. Cabinet Marquee Matrix".into(),
                        active_scene: "arcade_attract".into(),
                        brightness_pct: 95,
                    },
                ],
                fixtures: vec![
                    LightingFixture {
                        id: "wled-arch-01".into(),
                        name: "Cabinet Row Neon Canopy".into(),
                        zone: "arcade_floor".into(),
                        protocol: "WLED JSON API".into(),
                        endpoint: "http://10.10.20.51/json/state".into(),
                        universe: 1,
                        active_scene: "arcade_attract".into(),
                        brightness_pct: 90,
                        online: true,
                    },
                    LightingFixture {
                        id: "artnet-stage-01".into(),
                        name: "Main Floor DMX Moving Heads".into(),
                        zone: "tournament_stage".into(),
                        protocol: "Art-Net UDP:6454".into(),
                        endpoint: "10.10.20.255:6454".into(),
                        universe: 2,
                        active_scene: "cyberpunk_neon".into(),
                        brightness_pct: 100,
                        online: true,
                    },
                    LightingFixture {
                        id: "sacn-stage-02".into(),
                        name: "Stage Truss RGBW Blinders".into(),
                        zone: "tournament_stage".into(),
                        protocol: "sACN E1.31 UDP:5568".into(),
                        endpoint: "239.255.0.3:5568".into(),
                        universe: 3,
                        active_scene: "cyberpunk_neon".into(),
                        brightness_pct: 100,
                        online: true,
                    },
                    LightingFixture {
                        id: "wled-bar-02".into(),
                        name: "Bar Tap Backlight Strip".into(),
                        zone: "taproom_bar".into(),
                        protocol: "WLED JSON API".into(),
                        endpoint: "http://10.10.20.52/json/state".into(),
                        universe: 4,
                        active_scene: "chill_lounge".into(),
                        brightness_pct: 75,
                        online: true,
                    },
                    LightingFixture {
                        id: "hue-booths-03".into(),
                        name: "Taproom Booth Pendant Cluster".into(),
                        zone: "taproom_bar".into(),
                        protocol: "Philips Hue / Zigbee2MQTT".into(),
                        endpoint: "http://10.10.20.55/api/groups/booths".into(),
                        universe: 5,
                        active_scene: "chill_lounge".into(),
                        brightness_pct: 70,
                        online: true,
                    },
                    LightingFixture {
                        id: "wled-marquees-04".into(),
                        name: "O.R.I.O.N. Cabinet Marquee Bus".into(),
                        zone: "cabinet_marquees".into(),
                        protocol: "WLED DDP / UDP".into(),
                        endpoint: "http://10.10.20.58/json/state".into(),
                        universe: 6,
                        active_scene: "arcade_attract".into(),
                        brightness_pct: 95,
                        online: true,
                    },
                ],
                cue_history: VecDeque::from([LightingCueLog {
                    scene: "arcade_attract".into(),
                    zone: "ALL_ZONES".into(),
                    reason: "B.O.S.S. multi-controller boot default scene".into(),
                    triggered_at_ms: now_epoch_ms(),
                }]),
            }),
        }
    }

    fn push_cue(st: &mut LightingState, scene: &str, zone: &str, reason: String) {
        if st.cue_history.len() >= 30 {
            st.cue_history.pop_back();
        }
        st.cue_history.push_front(LightingCueLog {
            scene: scene.to_string(),
            zone: zone.to_string(),
            reason,
            triggered_at_ms: now_epoch_ms(),
        });
    }
}

#[axum::async_trait]
impl BossPlugin for VenueLightingPlugin {
    fn id(&self) -> &str {
        "venue-lighting"
    }

    async fn manifest(&self) -> PluginManifest {
        let enabled = self.inner.read().await.enabled;
        PluginManifest {
            id: "venue-lighting".into(),
            name: "Multi-System Venue Lighting (WLED / Art-Net DMX / sACN / Hue)".into(),
            version: "1.2.0".into(),
            category: PluginCategory::VenueLighting,
            description: "Multi-zone lighting orchestration across WLED, Art-Net DMX512, sACN E1.31, Philips Hue, and QLC+ OSC with reactive event cues.".into(),
            author: "B.E.E.R.C.A.D.E. Core".into(),
            enabled,
            builtin: true,
            subscribed_events: vec![
                "high_score".into(),
                "credit_transaction".into(),
                "node_offline".into(),
                "error_reported".into(),
            ],
            capabilities: vec![
                "multi_zone".into(),
                "dmx_artnet".into(),
                "sacn_e131".into(),
                "wled_http".into(),
                "philips_hue".into(),
                "reactive_cues".into(),
            ],
            ui_panel: PluginUiPanelSpec {
                tab_label: "Lighting & DMX".into(),
                icon: "sparkles".into(),
                summary_metric_key: "active_scene".into(),
            },
            webhook_url: None,
        }
    }

    async fn set_enabled(&self, enabled: bool) {
        self.inner.write().await.enabled = enabled;
    }

    async fn is_enabled(&self) -> bool {
        self.inner.read().await.enabled
    }

    async fn on_event(&self, event: &PluginEvent, _app_state: &SharedState) {
        let mut st = self.inner.write().await;
        if !st.enabled {
            return;
        }
        match event {
            PluginEvent::HighScore(entry) if st.strobe_on_high_score => {
                st.active_scene = "high_score_strobe".into();
                for z in st.zones.iter_mut() {
                    if z.id == "arcade_floor" || z.id == "tournament_stage" || z.id == "cabinet_marquees" {
                        z.active_scene = "high_score_strobe".into();
                    }
                }
                Self::push_cue(
                    &mut st,
                    "high_score_strobe",
                    "arcade_floor+stage+marquees",
                    format!(
                        "High Score {} by {} on {} ({})",
                        entry.score, entry.initials, entry.game_name, entry.node_id
                    ),
                );
            }
            PluginEvent::CreditTransaction(tx) if st.pulse_on_credit_insert => {
                Self::push_cue(
                    &mut st,
                    "coin_drop_ripple",
                    "cabinet_marquees",
                    format!("+{} credits injected on {}", tx.credits, tx.node_id),
                );
            }
            PluginEvent::NodeOffline { node_ids, .. } if st.red_alert_on_node_offline => {
                st.active_scene = "thermal_alert_red".into();
                Self::push_cue(
                    &mut st,
                    "thermal_alert_red",
                    "ALL_ZONES",
                    format!("Nodes went offline: {}", node_ids.join(", ")),
                );
            }
            PluginEvent::ErrorReported(err)
                if st.red_alert_on_node_offline && err.level == "fatal" =>
            {
                st.active_scene = "thermal_alert_red".into();
                Self::push_cue(
                    &mut st,
                    "thermal_alert_red",
                    "ALL_ZONES",
                    format!("Fatal error on {}: {}", err.node_id, err.message),
                );
            }
            _ => {}
        }
    }

    async fn invoke_action(
        &self,
        action: &str,
        payload: Value,
        _app_state: &SharedState,
    ) -> Result<Value, String> {
        let mut st = self.inner.write().await;
        match action {
            "set_scene" => {
                let scene = payload
                    .get("scene")
                    .and_then(Value::as_str)
                    .unwrap_or("arcade_attract")
                    .to_string();
                let zone_opt = payload.get("zone").and_then(Value::as_str);
                if let Some(b) = payload.get("brightness").and_then(Value::as_u64) {
                    st.master_brightness_pct = b.min(100) as u8;
                }
                let master_b = st.master_brightness_pct;
                if let Some(target_zone) = zone_opt.filter(|z| !z.is_empty() && *z != "ALL") {
                    for z in st.zones.iter_mut() {
                        if z.id == target_zone {
                            z.active_scene = scene.clone();
                            z.brightness_pct = master_b;
                        }
                    }
                    for f in st.fixtures.iter_mut() {
                        if f.zone == target_zone {
                            f.active_scene = scene.clone();
                            f.brightness_pct = master_b;
                        }
                    }
                    Self::push_cue(
                        &mut st,
                        &scene,
                        target_zone,
                        format!("Zone '{target_zone}' scene override"),
                    );
                } else {
                    st.active_scene = scene.clone();
                    for z in st.zones.iter_mut() {
                        z.active_scene = scene.clone();
                    }
                    for f in st.fixtures.iter_mut() {
                        f.active_scene = scene.clone();
                    }
                    Self::push_cue(&mut st, &scene, "ALL_ZONES", "Global operator scene override".into());
                }
                Ok(json!({
                    "ok": true,
                    "active_scene": st.active_scene,
                    "master_brightness_pct": st.master_brightness_pct,
                    "zones": st.zones,
                }))
            }
            "set_zone_scene" => {
                let zone_id = payload
                    .get("zone_id")
                    .and_then(Value::as_str)
                    .unwrap_or("arcade_floor")
                    .to_string();
                let scene = payload
                    .get("scene")
                    .and_then(Value::as_str)
                    .unwrap_or("arcade_attract")
                    .to_string();
                let brightness = payload
                    .get("brightness_pct")
                    .and_then(Value::as_u64)
                    .map(|b| b.min(100) as u8);

                for z in st.zones.iter_mut() {
                    if z.id == zone_id {
                        z.active_scene = scene.clone();
                        if let Some(b) = brightness {
                            z.brightness_pct = b;
                        }
                    }
                }
                for f in st.fixtures.iter_mut() {
                    if f.zone == zone_id {
                        f.active_scene = scene.clone();
                        if let Some(b) = brightness {
                            f.brightness_pct = b;
                        }
                    }
                }
                Self::push_cue(
                    &mut st,
                    &scene,
                    &zone_id,
                    format!("Zone '{zone_id}' updated to {scene}"),
                );
                Ok(json!({
                    "ok": true,
                    "zone_id": zone_id,
                    "scene": scene,
                    "zones": st.zones,
                }))
            }
            "trigger_effect" => {
                let effect = payload
                    .get("effect")
                    .and_then(Value::as_str)
                    .unwrap_or("jackpot_gold");
                let zone = payload
                    .get("zone")
                    .and_then(Value::as_str)
                    .unwrap_or("ALL_ZONES");
                Self::push_cue(&mut st, effect, zone, "Operator one-shot multi-controller burst".into());
                Ok(json!({ "ok": true, "effect": effect, "zone": zone }))
            }
            "toggle_rule" => {
                let rule = payload.get("rule").and_then(Value::as_str).unwrap_or("");
                let val = payload.get("enabled").and_then(Value::as_bool).unwrap_or(true);
                match rule {
                    "strobe_on_high_score" => st.strobe_on_high_score = val,
                    "pulse_on_credit_insert" => st.pulse_on_credit_insert = val,
                    "red_alert_on_node_offline" => st.red_alert_on_node_offline = val,
                    _ => return Err(format!("Unknown lighting rule '{rule}'")),
                }
                Ok(json!({ "ok": true, "rule": rule, "enabled": val }))
            }
            "add_fixture" => {
                let name = payload
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("New Lighting Controller")
                    .to_string();
                let zone = payload
                    .get("zone")
                    .and_then(Value::as_str)
                    .unwrap_or("arcade_floor")
                    .to_string();
                let protocol = payload
                    .get("protocol")
                    .and_then(Value::as_str)
                    .unwrap_or("WLED JSON API")
                    .to_string();
                let endpoint = payload
                    .get("endpoint")
                    .and_then(Value::as_str)
                    .unwrap_or("http://10.10.20.60/json/state")
                    .to_string();
                let universe = payload
                    .get("universe")
                    .and_then(Value::as_u64)
                    .unwrap_or(1) as u16;
                let fixture = LightingFixture {
                    id: format!("fix_{}", &uuid::Uuid::new_v4().to_string()[..6]),
                    name,
                    zone,
                    protocol,
                    endpoint,
                    universe,
                    active_scene: st.active_scene.clone(),
                    brightness_pct: st.master_brightness_pct,
                    online: true,
                };
                st.fixtures.push(fixture.clone());
                Ok(serde_json::to_value(fixture).unwrap_or_default())
            }
            "toggle_fixture" => {
                let fixture_id = payload.get("fixture_id").and_then(Value::as_str).unwrap_or("");
                let fix = st
                    .fixtures
                    .iter_mut()
                    .find(|f| f.id == fixture_id)
                    .ok_or_else(|| format!("Fixture '{fixture_id}' not found"))?;
                fix.online = !fix.online;
                Ok(serde_json::to_value(fix.clone()).unwrap_or_default())
            }
            _ => Err(format!("Unsupported action '{action}' for venue-lighting")),
        }
    }

    async fn state_snapshot(&self) -> Value {
        let st = self.inner.read().await;
        json!({
            "active_scene": st.active_scene,
            "master_brightness_pct": st.master_brightness_pct,
            "available_scenes": [
                { "id": "arcade_attract", "label": "Arcade Attract (Cyan/Magenta)", "color": "#00f0ff" },
                { "id": "cyberpunk_neon", "label": "Cyberpunk Synthwave", "color": "#ff0055" },
                { "id": "high_score_strobe", "label": "High Score Strobe Burst", "color": "#ffe600" },
                { "id": "jackpot_gold", "label": "Jackpot Gold Chase", "color": "#ffaa00" },
                { "id": "thermal_alert_red", "label": "Ops / Thermal Alert Red", "color": "#ff2222" },
                { "id": "chill_lounge", "label": "Late Night Warm Lounge", "color": "#9d4edd" }
            ],
            "reactive_rules": {
                "strobe_on_high_score": st.strobe_on_high_score,
                "pulse_on_credit_insert": st.pulse_on_credit_insert,
                "red_alert_on_node_offline": st.red_alert_on_node_offline,
            },
            "zones": st.zones,
            "fixtures": st.fixtures,
            "cue_history": st.cue_history,
        })
    }
}

// ---------------------------------------------------------------------------
// 3. Tournament, High-Score Leaderboard & TV Attractor Signage (`tournament-engine`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TournamentStanding {
    pub initials: String,
    pub player_name: String,
    pub score: u64,
    pub node_id: String,
    pub achieved_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BracketMatch {
    pub id: String,
    pub round: u8,
    pub player_a: String,
    pub player_b: String,
    pub winner: Option<String>,
    pub cabinet_node_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TvSignageDisplay {
    pub id: String,
    pub name: String,
    pub zone: String,
    pub stream_url: String,
    pub output_mode: String,
    pub slide_interval_secs: u32,
    pub active_slides: Vec<String>,
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TvFanfareAlert {
    pub id: String,
    pub kind: String,
    pub headline: String,
    pub subtext: String,
    pub node_id: String,
    pub triggered_at_ms: u64,
    pub duration_secs: u32,
}

struct TournamentState {
    enabled: bool,
    title: String,
    game_name: String,
    target_rom: String,
    mode: String,
    status: String,
    prize_pool: String,
    kiosk_overlay_enabled: bool,
    tv_announcement_banner: String,
    tv_slide_interval_secs: u32,
    tv_displays: Vec<TvSignageDisplay>,
    active_fanfare: Option<TvFanfareAlert>,
    registered_players: Vec<String>,
    leaderboard: Vec<TournamentStanding>,
    bracket_matches: Vec<BracketMatch>,
}

pub struct TournamentEnginePlugin {
    inner: RwLock<TournamentState>,
}

impl TournamentEnginePlugin {
    pub fn new() -> Self {
        let now = now_epoch_ms();
        Self {
            inner: RwLock::new(TournamentState {
                enabled: true,
                title: "FRIDAY NIGHT FIGHTS: STREET FIGHTER III 3RD STRIKE".into(),
                game_name: "Street Fighter III: 3rd Strike".into(),
                target_rom: "sfiii3".into(),
                mode: "single_elimination".into(),
                status: "live".into(),
                prize_pool: "$150 Bar Tab + Champion Belt + 50 Tokens".into(),
                kiosk_overlay_enabled: true,
                tv_announcement_banner: "SCAN QR ON ANY TV TO BUY A.S.T.R.O. GAME TIME VIA SQUARE · HAPPY HOUR $8 PINTS UNTIL 7PM".into(),
                tv_slide_interval_secs: 12,
                tv_displays: vec![
                    TvSignageDisplay {
                        id: "tv-main-stage".into(),
                        name: "Main Floor 75\" High-Score Wall".into(),
                        zone: "main".into(),
                        stream_url: "/tv?zone=main".into(),
                        output_mode: "B.O.S.S. HDMI-1 + LAN Web Stream".into(),
                        slide_interval_secs: 12,
                        active_slides: vec![
                            "high_scores".into(),
                            "tournament_bracket".into(),
                            "live_cabinets".into(),
                            "taproom_promo".into(),
                        ],
                        online: true,
                    },
                    TvSignageDisplay {
                        id: "tv-taproom-bar".into(),
                        name: "Taproom Bar Overhead 55\" Signage".into(),
                        zone: "bar".into(),
                        stream_url: "/tv?zone=bar".into(),
                        output_mode: "Smart TV LAN Kiosk Browser".into(),
                        slide_interval_secs: 15,
                        active_slides: vec![
                            "taproom_promo".into(),
                            "high_scores".into(),
                            "live_cabinets".into(),
                        ],
                        online: true,
                    },
                    TvSignageDisplay {
                        id: "tv-stage-rig".into(),
                        name: "Tournament Stage Spectator Projector".into(),
                        zone: "stage".into(),
                        stream_url: "/tv?zone=stage".into(),
                        output_mode: "Smart TV LAN Kiosk Browser".into(),
                        slide_interval_secs: 10,
                        active_slides: vec![
                            "tournament_bracket".into(),
                            "high_scores".into(),
                        ],
                        online: true,
                    },
                ],
                active_fanfare: None,
                registered_players: vec![
                    "DAIGO (DGO)".into(),
                    "JUSTIN (JWong)".into(),
                    "KURODA (KRD)".into(),
                    "MOV (MOV)".into(),
                ],
                leaderboard: vec![
                    TournamentStanding {
                        initials: "DGO".into(),
                        player_name: "DAIGO".into(),
                        score: 984_200,
                        node_id: "ASTRO-01".into(),
                        achieved_at_ms: now.saturating_sub(600_000),
                    },
                    TournamentStanding {
                        initials: "JWG".into(),
                        player_name: "JUSTIN".into(),
                        score: 912_500,
                        node_id: "ASTRO-02".into(),
                        achieved_at_ms: now.saturating_sub(420_000),
                    },
                    TournamentStanding {
                        initials: "KRD".into(),
                        player_name: "KURODA".into(),
                        score: 875_000,
                        node_id: "ASTRO-01".into(),
                        achieved_at_ms: now.saturating_sub(180_000),
                    },
                ],
                bracket_matches: vec![
                    BracketMatch {
                        id: "m-r1-1".into(),
                        round: 1,
                        player_a: "DAIGO (DGO)".into(),
                        player_b: "JUSTIN (JWong)".into(),
                        winner: Some("DAIGO (DGO)".into()),
                        cabinet_node_id: "ASTRO-01".into(),
                        status: "completed".into(),
                    },
                    BracketMatch {
                        id: "m-r1-2".into(),
                        round: 1,
                        player_a: "KURODA (KRD)".into(),
                        player_b: "MOV (MOV)".into(),
                        winner: None,
                        cabinet_node_id: "ASTRO-02".into(),
                        status: "in_progress".into(),
                    },
                ],
            }),
        }
    }
}

#[axum::async_trait]
impl BossPlugin for TournamentEnginePlugin {
    fn id(&self) -> &str {
        "tournament-engine"
    }

    async fn manifest(&self) -> PluginManifest {
        let enabled = self.inner.read().await.enabled;
        PluginManifest {
            id: "tournament-engine".into(),
            name: "Tournament, High-Score & TV Attractor Engine".into(),
            version: "1.2.0".into(),
            category: PluginCategory::TournamentEngine,
            description: "Bracket management, real-time high-score leaderboards, and on-site TV attractor signage streaming (/tv).".into(),
            author: "B.E.E.R.C.A.D.E. Core".into(),
            enabled,
            builtin: true,
            subscribed_events: vec!["high_score".into()],
            capabilities: vec![
                "brackets".into(),
                "score_attack".into(),
                "kiosk_attractor_overlay".into(),
                "tv_signage_stream".into(),
            ],
            ui_panel: PluginUiPanelSpec {
                tab_label: "Tournaments & TV".into(),
                icon: "trophy".into(),
                summary_metric_key: "status".into(),
            },
            webhook_url: None,
        }
    }

    async fn set_enabled(&self, enabled: bool) {
        self.inner.write().await.enabled = enabled;
    }

    async fn is_enabled(&self) -> bool {
        self.inner.read().await.enabled
    }

    async fn on_event(&self, event: &PluginEvent, _app_state: &SharedState) {
        if let PluginEvent::HighScore(entry) = event {
            let mut st = self.inner.write().await;
            if !st.enabled {
                return;
            }
            // Always trigger a live TV fanfare interrupt when a high score arrives
            st.active_fanfare = Some(TvFanfareAlert {
                id: format!("fan_{}", &uuid::Uuid::new_v4().to_string()[..6]),
                kind: "high_score".into(),
                headline: format!("NEW HIGH SCORE BY {}!", entry.initials),
                subtext: format!(
                    "{} PTS ON {} ({})",
                    entry.score, entry.game_name, entry.node_id
                ),
                node_id: entry.node_id.clone(),
                triggered_at_ms: now_epoch_ms(),
                duration_secs: 8,
            });

            if st.status != "live" {
                return;
            }
            let rom_matches = st.target_rom == "*"
                || entry
                    .active_rom
                    .to_ascii_lowercase()
                    .contains(&st.target_rom.to_ascii_lowercase());
            if rom_matches {
                st.leaderboard.push(TournamentStanding {
                    initials: entry.initials.clone(),
                    player_name: entry.initials.clone(),
                    score: entry.score,
                    node_id: entry.node_id.clone(),
                    achieved_at_ms: entry.achieved_at_epoch_ms,
                });
                st.leaderboard.sort_by(|a, b| b.score.cmp(&a.score));
                st.leaderboard.truncate(20);
            }
        }
    }

    async fn invoke_action(
        &self,
        action: &str,
        payload: Value,
        _app_state: &SharedState,
    ) -> Result<Value, String> {
        let mut st = self.inner.write().await;
        match action {
            "configure_tournament" => {
                if let Some(v) = payload.get("title").and_then(Value::as_str) {
                    st.title = v.to_string();
                }
                if let Some(v) = payload.get("game_name").and_then(Value::as_str) {
                    st.game_name = v.to_string();
                }
                if let Some(v) = payload.get("target_rom").and_then(Value::as_str) {
                    st.target_rom = v.to_string();
                }
                if let Some(v) = payload.get("mode").and_then(Value::as_str) {
                    st.mode = v.to_string();
                }
                if let Some(v) = payload.get("prize_pool").and_then(Value::as_str) {
                    st.prize_pool = v.to_string();
                }
                if let Some(v) = payload.get("status").and_then(Value::as_str) {
                    st.status = v.to_string();
                }
                Ok(json!({ "ok": true, "title": st.title, "status": st.status }))
            }
            "update_tv_config" => {
                if let Some(banner) = payload.get("tv_announcement_banner").and_then(Value::as_str) {
                    st.tv_announcement_banner = banner.to_string();
                }
                if let Some(interval) = payload.get("tv_slide_interval_secs").and_then(Value::as_u64) {
                    let secs = interval.clamp(5, 120) as u32;
                    st.tv_slide_interval_secs = secs;
                    for d in st.tv_displays.iter_mut() {
                        d.slide_interval_secs = secs;
                    }
                }
                Ok(json!({
                    "ok": true,
                    "tv_announcement_banner": st.tv_announcement_banner,
                    "tv_slide_interval_secs": st.tv_slide_interval_secs,
                    "tv_displays": st.tv_displays,
                }))
            }
            "trigger_tv_fanfare" => {
                let headline = payload
                    .get("headline")
                    .and_then(Value::as_str)
                    .unwrap_or("NEW HIGH SCORE RECORD!")
                    .to_string();
                let subtext = payload
                    .get("subtext")
                    .and_then(Value::as_str)
                    .unwrap_or("DAIGO (DGO) — 999,900 PTS ON ASTRO-01")
                    .to_string();
                let kind = payload
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("high_score")
                    .to_string();
                let node_id = payload
                    .get("node_id")
                    .and_then(Value::as_str)
                    .unwrap_or("ASTRO-01")
                    .to_string();
                let fanfare = TvFanfareAlert {
                    id: format!("fan_{}", &uuid::Uuid::new_v4().to_string()[..6]),
                    kind,
                    headline,
                    subtext,
                    node_id,
                    triggered_at_ms: now_epoch_ms(),
                    duration_secs: 8,
                };
                st.active_fanfare = Some(fanfare.clone());
                Ok(json!({ "ok": true, "fanfare": fanfare }))
            }
            "register_player" => {
                let player = payload
                    .get("player")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if player.is_empty() {
                    return Err("Player name must not be empty".into());
                }
                if !st.registered_players.contains(&player) {
                    st.registered_players.push(player.clone());
                }
                Ok(json!({ "ok": true, "registered_players": st.registered_players }))
            }
            "generate_bracket" => {
                let mut matches = Vec::new();
                let players = st.registered_players.clone();
                for (idx, pair) in players.chunks(2).enumerate() {
                    let player_a = pair.first().cloned().unwrap_or_else(|| "BYE".into());
                    let player_b = pair.get(1).cloned().unwrap_or_else(|| "BYE".into());
                    let winner = if player_b == "BYE" {
                        Some(player_a.clone())
                    } else {
                        None
                    };
                    matches.push(BracketMatch {
                        id: format!("m-r1-{}", idx + 1),
                        round: 1,
                        player_a,
                        player_b,
                        status: if winner.is_some() {
                            "completed".into()
                        } else {
                            "in_progress".into()
                        },
                        winner,
                        cabinet_node_id: format!("ASTRO-{:02}", (idx % 4) + 1),
                    });
                }
                st.bracket_matches = matches;
                st.status = "live".into();
                Ok(json!({ "ok": true, "bracket_matches": st.bracket_matches }))
            }
            "report_match_winner" => {
                let match_id = payload
                    .get("match_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let winner = payload
                    .get("winner")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                for m in st.bracket_matches.iter_mut() {
                    if m.id == match_id {
                        m.winner = Some(winner.clone());
                        m.status = "completed".into();
                    }
                }
                st.active_fanfare = Some(TvFanfareAlert {
                    id: format!("fan_{}", &uuid::Uuid::new_v4().to_string()[..6]),
                    kind: "tournament_win".into(),
                    headline: format!("MATCH WINNER: {winner}!"),
                    subtext: format!("Advances in {}", st.title),
                    node_id: "STAGE".into(),
                    triggered_at_ms: now_epoch_ms(),
                    duration_secs: 8,
                });
                Ok(json!({ "ok": true, "match_id": match_id, "winner": winner }))
            }
            "toggle_kiosk_overlay" => {
                let enabled = payload
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(!st.kiosk_overlay_enabled);
                st.kiosk_overlay_enabled = enabled;
                Ok(json!({ "ok": true, "kiosk_overlay_enabled": st.kiosk_overlay_enabled }))
            }
            _ => Err(format!("Unsupported action '{action}' for tournament-engine")),
        }
    }

    async fn state_snapshot(&self) -> Value {
        let st = self.inner.read().await;
        json!({
            "title": st.title,
            "game_name": st.game_name,
            "target_rom": st.target_rom,
            "mode": st.mode,
            "status": st.status,
            "prize_pool": st.prize_pool,
            "kiosk_overlay_enabled": st.kiosk_overlay_enabled,
            "tv_announcement_banner": st.tv_announcement_banner,
            "tv_slide_interval_secs": st.tv_slide_interval_secs,
            "tv_displays": st.tv_displays,
            "active_fanfare": st.active_fanfare,
            "registered_players": st.registered_players,
            "leaderboard": st.leaderboard,
            "bracket_matches": st.bracket_matches,
        })
    }
}

// ---------------------------------------------------------------------------
// 4. RFID / NFC Wristband & Card Peripherals (`rfid-peripherals`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfidCardRecord {
    pub uid: String,
    pub holder_name: String,
    pub initials: String,
    pub credits_balance: u32,
    pub vip_freeplay: bool,
    pub active: bool,
    pub last_tap_node: Option<String>,
    pub last_tap_ms: Option<u64>,
    pub total_taps: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfidTapEvent {
    pub uid: String,
    pub holder_name: String,
    pub node_id: String,
    pub credits_injected: u32,
    pub remaining_balance: u32,
    pub vip_freeplay: bool,
    pub tapped_at_ms: u64,
}

struct RfidState {
    enabled: bool,
    credits_per_tap: u32,
    cards: Vec<RfidCardRecord>,
    recent_taps: VecDeque<RfidTapEvent>,
}

pub struct RfidPeripheralsPlugin {
    inner: RwLock<RfidState>,
}

impl RfidPeripheralsPlugin {
    pub fn new() -> Self {
        let now = now_epoch_ms();
        Self {
            inner: RwLock::new(RfidState {
                enabled: true,
                credits_per_tap: 1,
                cards: vec![
                    RfidCardRecord {
                        uid: "04:A1:9B:22:C8:10:80".into(),
                        holder_name: "Venue VIP Wristband #01".into(),
                        initials: "VIP".into(),
                        credits_balance: 99,
                        vip_freeplay: true,
                        active: true,
                        last_tap_node: Some("ASTRO-01".into()),
                        last_tap_ms: Some(now.saturating_sub(120_000)),
                        total_taps: 42,
                    },
                    RfidCardRecord {
                        uid: "04:7F:3C:11:90:55:80".into(),
                        holder_name: "Patron Card #104 (Alex)".into(),
                        initials: "ALX".into(),
                        credits_balance: 14,
                        vip_freeplay: false,
                        active: true,
                        last_tap_node: Some("ASTRO-02".into()),
                        last_tap_ms: Some(now.saturating_sub(300_000)),
                        total_taps: 11,
                    },
                    RfidCardRecord {
                        uid: "04:E2:44:8A:19:02:80".into(),
                        holder_name: "Tournament Pass #08".into(),
                        initials: "TRN".into(),
                        credits_balance: 25,
                        vip_freeplay: false,
                        active: true,
                        last_tap_node: None,
                        last_tap_ms: None,
                        total_taps: 0,
                    },
                ],
                recent_taps: VecDeque::new(),
            }),
        }
    }
}

#[axum::async_trait]
impl BossPlugin for RfidPeripheralsPlugin {
    fn id(&self) -> &str {
        "rfid-peripherals"
    }

    async fn manifest(&self) -> PluginManifest {
        let enabled = self.inner.read().await.enabled;
        PluginManifest {
            id: "rfid-peripherals".into(),
            name: "RFID / NFC Wristband & Card Peripherals".into(),
            version: "1.0.0".into(),
            category: PluginCategory::RfidPeripherals,
            description: "Tap-to-credit NFC wristband readers, player profile & high-score initials lookup, and VIP free-play passes.".into(),
            author: "B.E.E.R.C.A.D.E. Core".into(),
            enabled,
            builtin: true,
            subscribed_events: vec!["credit_transaction".into()],
            capabilities: vec![
                "nfc_tap_to_play".into(),
                "vip_freeplay".into(),
                "card_topup".into(),
            ],
            ui_panel: PluginUiPanelSpec {
                tab_label: "RFID & Wristbands".into(),
                icon: "wifi".into(),
                summary_metric_key: "active_cards_count".into(),
            },
            webhook_url: None,
        }
    }

    async fn set_enabled(&self, enabled: bool) {
        self.inner.write().await.enabled = enabled;
    }

    async fn is_enabled(&self) -> bool {
        self.inner.read().await.enabled
    }

    async fn on_event(&self, _event: &PluginEvent, _app_state: &SharedState) {}

    async fn invoke_action(
        &self,
        action: &str,
        payload: Value,
        app_state: &SharedState,
    ) -> Result<Value, String> {
        match action {
            "tap_card" => {
                let uid = payload
                    .get("uid")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let node_id = payload
                    .get("node_id")
                    .and_then(Value::as_str)
                    .unwrap_or("ASTRO-01")
                    .trim()
                    .to_string();

                let (credits_to_add, tap_event) = {
                    let mut st = self.inner.write().await;
                    let cost = st.credits_per_tap.max(1);
                    let card = st
                        .cards
                        .iter_mut()
                        .find(|c| c.uid.eq_ignore_ascii_case(&uid))
                        .ok_or_else(|| format!("Unregistered RFID UID '{uid}'"))?;

                    if !card.active {
                        return Err("Card is deactivated".into());
                    }
                    if !card.vip_freeplay && card.credits_balance < cost {
                        return Err(format!(
                            "Insufficient wristband balance ({} credits remaining)",
                            card.credits_balance
                        ));
                    }
                    if !card.vip_freeplay {
                        card.credits_balance -= cost;
                    }
                    card.last_tap_node = Some(node_id.clone());
                    card.last_tap_ms = Some(now_epoch_ms());
                    card.total_taps += 1;

                    let ev = RfidTapEvent {
                        uid: card.uid.clone(),
                        holder_name: card.holder_name.clone(),
                        node_id: node_id.clone(),
                        credits_injected: cost,
                        remaining_balance: card.credits_balance,
                        vip_freeplay: card.vip_freeplay,
                        tapped_at_ms: now_epoch_ms(),
                    };
                    if st.recent_taps.len() >= 40 {
                        st.recent_taps.pop_back();
                    }
                    st.recent_taps.push_front(ev.clone());
                    (cost, ev)
                };

                let cmd = app_state
                    .enqueue_command(
                        &node_id,
                        CabinetCommand::AddCredits {
                            credits: credits_to_add,
                        },
                    )
                    .await;

                let tx = TokenTransaction {
                    node_id: node_id.clone(),
                    kind: "rfid_tap".into(),
                    credits: credits_to_add,
                    balance_after: tap_event.remaining_balance,
                    command_id: Some(cmd.id.clone()),
                    created_at_epoch_ms: now_epoch_ms(),
                };
                app_state.record_transaction(tx.clone()).await;
                app_state.buffer_edge_item(EdgeItem::Transaction(tx)).await;

                Ok(json!({
                    "ok": true,
                    "tap": tap_event,
                    "command_id": cmd.id
                }))
            }
            "register_card" => {
                let uid = payload
                    .get("uid")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_uppercase();
                if uid.is_empty() {
                    return Err("UID is required".into());
                }
                let holder_name = payload
                    .get("holder_name")
                    .and_then(Value::as_str)
                    .unwrap_or("Guest Wristband")
                    .to_string();
                let initials = payload
                    .get("initials")
                    .and_then(Value::as_str)
                    .unwrap_or("GST")
                    .chars()
                    .take(3)
                    .collect::<String>()
                    .to_uppercase();
                let credits_balance = payload
                    .get("credits_balance")
                    .and_then(Value::as_u64)
                    .unwrap_or(10) as u32;
                let vip_freeplay = payload
                    .get("vip_freeplay")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);

                let mut st = self.inner.write().await;
                if let Some(existing) = st.cards.iter_mut().find(|c| c.uid == uid) {
                    existing.holder_name = holder_name;
                    existing.initials = initials;
                    existing.credits_balance = credits_balance;
                    existing.vip_freeplay = vip_freeplay;
                    existing.active = true;
                    return Ok(serde_json::to_value(existing.clone()).unwrap_or_default());
                }

                let card = RfidCardRecord {
                    uid,
                    holder_name,
                    initials,
                    credits_balance,
                    vip_freeplay,
                    active: true,
                    last_tap_node: None,
                    last_tap_ms: None,
                    total_taps: 0,
                };
                st.cards.push(card.clone());
                Ok(serde_json::to_value(card).unwrap_or_default())
            }
            "topup_card" => {
                let uid = payload.get("uid").and_then(Value::as_str).unwrap_or("");
                let add_credits = payload
                    .get("credits")
                    .and_then(Value::as_u64)
                    .unwrap_or(5)
                    .clamp(1, 500) as u32;
                let mut st = self.inner.write().await;
                let card = st
                    .cards
                    .iter_mut()
                    .find(|c| c.uid.eq_ignore_ascii_case(uid))
                    .ok_or_else(|| format!("Card '{uid}' not found"))?;
                card.credits_balance = card.credits_balance.saturating_add(add_credits);
                Ok(serde_json::to_value(card.clone()).unwrap_or_default())
            }
            "toggle_vip" => {
                let uid = payload.get("uid").and_then(Value::as_str).unwrap_or("");
                let mut st = self.inner.write().await;
                let card = st
                    .cards
                    .iter_mut()
                    .find(|c| c.uid.eq_ignore_ascii_case(uid))
                    .ok_or_else(|| format!("Card '{uid}' not found"))?;
                card.vip_freeplay = !card.vip_freeplay;
                Ok(serde_json::to_value(card.clone()).unwrap_or_default())
            }
            _ => Err(format!("Unsupported action '{action}' for rfid-peripherals")),
        }
    }

    async fn state_snapshot(&self) -> Value {
        let st = self.inner.read().await;
        json!({
            "active_cards_count": st.cards.iter().filter(|c| c.active).count(),
            "credits_per_tap": st.credits_per_tap,
            "cards": st.cards,
            "recent_taps": st.recent_taps,
        })
    }
}

// ---------------------------------------------------------------------------
// 5. Alerting & Ops Sinks Plugin (`alerting-sinks`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertSinkConfig {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub webhook_url: String,
    pub enabled: bool,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertDeliveryRecord {
    pub id: String,
    pub sink_name: String,
    pub severity: String,
    pub title: String,
    pub message: String,
    pub node_id: String,
    pub delivered_at_ms: u64,
    pub status: String,
}

struct AlertingState {
    enabled: bool,
    thermal_warn_temp_c: f32,
    sinks: Vec<AlertSinkConfig>,
    deliveries: VecDeque<AlertDeliveryRecord>,
    last_thermal_alert_by_node: HashMap<String, u64>,
}

pub struct AlertingSinksPlugin {
    inner: RwLock<AlertingState>,
}

impl AlertingSinksPlugin {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(AlertingState {
                enabled: true,
                thermal_warn_temp_c: 78.0,
                sinks: vec![
                    AlertSinkConfig {
                        id: "sink-discord-ops".into(),
                        name: "Discord #venue-ops-alerts".into(),
                        kind: "discord".into(),
                        webhook_url: "https://discord.com/api/webhooks/beercade-ops/simulated".into(),
                        enabled: true,
                        events: vec!["error".into(), "fatal".into(), "thermal".into(), "offline".into()],
                    },
                    AlertSinkConfig {
                        id: "sink-slack-tech".into(),
                        name: "Slack #cabinet-techs".into(),
                        kind: "slack".into(),
                        webhook_url: "https://hooks.slack.com/services/T000/B000/beercade".into(),
                        enabled: true,
                        events: vec!["fatal".into(), "offline".into()],
                    },
                    AlertSinkConfig {
                        id: "sink-pagerduty".into(),
                        name: "PagerDuty On-Call Duty Engineer".into(),
                        kind: "pagerduty".into(),
                        webhook_url: "https://events.pagerduty.com/v2/enqueue".into(),
                        enabled: false,
                        events: vec!["fatal".into()],
                    },
                ],
                deliveries: VecDeque::new(),
                last_thermal_alert_by_node: HashMap::new(),
            }),
        }
    }

    fn record_delivery(
        st: &mut AlertingState,
        severity: &str,
        title: String,
        message: String,
        node_id: &str,
    ) {
        let now = now_epoch_ms();
        let active_sinks: Vec<String> = st
            .sinks
            .iter()
            .filter(|s| s.enabled)
            .map(|s| s.name.clone())
            .collect();

        for sink_name in active_sinks {
            if st.deliveries.len() >= 50 {
                st.deliveries.pop_back();
            }
            st.deliveries.push_front(AlertDeliveryRecord {
                id: format!("alrt_{}", &uuid::Uuid::new_v4().to_string()[..8]),
                sink_name,
                severity: severity.to_string(),
                title: title.clone(),
                message: message.clone(),
                node_id: node_id.to_string(),
                delivered_at_ms: now,
                status: "delivered (200 OK)".into(),
            });
        }
    }
}

#[axum::async_trait]
impl BossPlugin for AlertingSinksPlugin {
    fn id(&self) -> &str {
        "alerting-sinks"
    }

    async fn manifest(&self) -> PluginManifest {
        let enabled = self.inner.read().await.enabled;
        PluginManifest {
            id: "alerting-sinks".into(),
            name: "Alerting & Ops Sinks".into(),
            version: "1.0.0".into(),
            category: PluginCategory::AlertingSinks,
            description: "Discord, Slack, PagerDuty, and Telegram webhook routing for cabinet errors, thermal warnings, and offline nodes.".into(),
            author: "B.E.E.R.C.A.D.E. Core".into(),
            enabled,
            builtin: true,
            subscribed_events: vec![
                "telemetry_heartbeat".into(),
                "error_reported".into(),
                "node_offline".into(),
            ],
            capabilities: vec![
                "discord_webhook".into(),
                "slack_webhook".into(),
                "pagerduty_events".into(),
                "thermal_watchdog".into(),
            ],
            ui_panel: PluginUiPanelSpec {
                tab_label: "Alerting & Sinks".into(),
                icon: "bell".into(),
                summary_metric_key: "deliveries_count".into(),
            },
            webhook_url: None,
        }
    }

    async fn set_enabled(&self, enabled: bool) {
        self.inner.write().await.enabled = enabled;
    }

    async fn is_enabled(&self) -> bool {
        self.inner.read().await.enabled
    }

    async fn on_event(&self, event: &PluginEvent, _app_state: &SharedState) {
        let mut st = self.inner.write().await;
        if !st.enabled {
            return;
        }
        match event {
            PluginEvent::TelemetryHeartbeat(hb) => {
                let max_temp = hb.cpu_temp_c.max(hb.gpu_temp_c);
                if max_temp >= st.thermal_warn_temp_c {
                    let now = now_epoch_ms();
                    let last = st
                        .last_thermal_alert_by_node
                        .get(&hb.node_id)
                        .copied()
                        .unwrap_or(0);
                    // 60-second per-node cooldown
                    if now.saturating_sub(last) >= 60_000 {
                        st.last_thermal_alert_by_node.insert(hb.node_id.clone(), now);
                        let threshold = st.thermal_warn_temp_c;
                        Self::record_delivery(
                            &mut st,
                            "thermal",
                            format!("Thermal Warning on {}", hb.node_id),
                            format!(
                                "CPU {:.1}°C / GPU {:.1}°C exceeded {:.1}°C threshold",
                                hb.cpu_temp_c, hb.gpu_temp_c, threshold
                            ),
                            &hb.node_id,
                        );
                    }
                }
            }
            PluginEvent::ErrorReported(err) => {
                Self::record_delivery(
                    &mut st,
                    &err.level,
                    format!("Cabinet {} [{}]", err.node_id, err.level.to_uppercase()),
                    format!("{}: {}", err.source, err.message),
                    &err.node_id,
                );
            }
            PluginEvent::NodeOffline { node_ids, .. } => {
                let joined = node_ids.join(", ");
                Self::record_delivery(
                    &mut st,
                    "offline",
                    "Cabinet Heartbeat Lost".into(),
                    format!("Nodes went offline (>15s stale): {joined}"),
                    &joined,
                );
            }
            _ => {}
        }
    }

    async fn invoke_action(
        &self,
        action: &str,
        payload: Value,
        _app_state: &SharedState,
    ) -> Result<Value, String> {
        let mut st = self.inner.write().await;
        match action {
            "send_test_alert" => {
                let message = payload
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Synthetic operator test ping from B.O.S.S. Management Console")
                    .to_string();
                Self::record_delivery(
                    &mut st,
                    "info",
                    "B.O.S.S. Operator Test Alert".into(),
                    message,
                    "BOSS-CENTRAL",
                );
                Ok(json!({ "ok": true, "deliveries_count": st.deliveries.len() }))
            }
            "set_thermal_threshold" => {
                let temp = payload
                    .get("thermal_warn_temp_c")
                    .and_then(Value::as_f64)
                    .unwrap_or(78.0)
                    .clamp(45.0, 105.0) as f32;
                st.thermal_warn_temp_c = temp;
                Ok(json!({ "ok": true, "thermal_warn_temp_c": st.thermal_warn_temp_c }))
            }
            "toggle_sink" => {
                let sink_id = payload.get("sink_id").and_then(Value::as_str).unwrap_or("");
                let sink = st
                    .sinks
                    .iter_mut()
                    .find(|s| s.id == sink_id)
                    .ok_or_else(|| format!("Sink '{sink_id}' not found"))?;
                sink.enabled = !sink.enabled;
                Ok(serde_json::to_value(sink.clone()).unwrap_or_default())
            }
            "add_sink" => {
                let name = payload
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Custom Webhook Sink")
                    .to_string();
                let kind = payload
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("discord")
                    .to_string();
                let webhook_url = payload
                    .get("webhook_url")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if webhook_url.is_empty() {
                    return Err("webhook_url is required".into());
                }
                let sink = AlertSinkConfig {
                    id: format!("sink_{}", &uuid::Uuid::new_v4().to_string()[..6]),
                    name,
                    kind,
                    webhook_url,
                    enabled: true,
                    events: vec!["error".into(), "fatal".into(), "thermal".into(), "offline".into()],
                };
                st.sinks.push(sink.clone());
                Ok(serde_json::to_value(sink).unwrap_or_default())
            }
            "clear_history" => {
                st.deliveries.clear();
                Ok(json!({ "ok": true }))
            }
            _ => Err(format!("Unsupported action '{action}' for alerting-sinks")),
        }
    }

    async fn state_snapshot(&self) -> Value {
        let st = self.inner.read().await;
        json!({
            "thermal_warn_temp_c": st.thermal_warn_temp_c,
            "deliveries_count": st.deliveries.len(),
            "sinks": st.sinks,
            "deliveries": st.deliveries,
        })
    }
}

// ---------------------------------------------------------------------------
// Out-of-Tree / Dynamic Webhook Plugin (`WebhookPlugin`)
// ---------------------------------------------------------------------------

struct WebhookPluginInner {
    manifest: PluginManifest,
    dispatched_count: u64,
    last_event_name: Option<String>,
    last_dispatch_ms: Option<u64>,
    last_status: String,
}

pub struct WebhookPlugin {
    inner: RwLock<WebhookPluginInner>,
    client: reqwest::Client,
}

impl WebhookPlugin {
    pub fn new(mut manifest: PluginManifest) -> Self {
        manifest.builtin = false;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        Self {
            inner: RwLock::new(WebhookPluginInner {
                manifest,
                dispatched_count: 0,
                last_event_name: None,
                last_dispatch_ms: None,
                last_status: "ready".into(),
            }),
            client,
        }
    }
}

#[axum::async_trait]
impl BossPlugin for WebhookPlugin {
    fn id(&self) -> &str {
        "webhook-custom"
    }

    async fn manifest(&self) -> PluginManifest {
        self.inner.read().await.manifest.clone()
    }

    async fn set_enabled(&self, enabled: bool) {
        self.inner.write().await.manifest.enabled = enabled;
    }

    async fn is_enabled(&self) -> bool {
        self.inner.read().await.manifest.enabled
    }

    async fn on_event(&self, event: &PluginEvent, _app_state: &SharedState) {
        let ev_name = event.event_name();
        let (enabled, subscribed, url_opt, plugin_id) = {
            let st = self.inner.read().await;
            (
                st.manifest.enabled,
                st.manifest
                    .subscribed_events
                    .iter()
                    .any(|e| e == ev_name || e == "*"),
                st.manifest.webhook_url.clone(),
                st.manifest.id.clone(),
            )
        };
        if !enabled || !subscribed {
            return;
        }
        let Some(url) = url_opt else {
            return;
        };

        let body = json!({
            "plugin_id": plugin_id,
            "event": event,
            "dispatched_at_epoch_ms": now_epoch_ms(),
        });

        let status_str = match self.client.post(&url).json(&body).send().await {
            Ok(resp) => format!("HTTP {}", resp.status().as_u16()),
            Err(e) => format!("error: {e}"),
        };

        let mut st = self.inner.write().await;
        st.dispatched_count += 1;
        st.last_event_name = Some(ev_name.to_string());
        st.last_dispatch_ms = Some(now_epoch_ms());
        st.last_status = status_str;
    }

    async fn invoke_action(
        &self,
        action: &str,
        payload: Value,
        _app_state: &SharedState,
    ) -> Result<Value, String> {
        let mut st = self.inner.write().await;
        st.dispatched_count += 1;
        st.last_event_name = Some(format!("action:{action}"));
        st.last_dispatch_ms = Some(now_epoch_ms());
        Ok(json!({
            "ok": true,
            "plugin_id": st.manifest.id,
            "action": action,
            "echo": payload
        }))
    }

    async fn state_snapshot(&self) -> Value {
        let st = self.inner.read().await;
        json!({
            "dispatched_count": st.dispatched_count,
            "last_event_name": st.last_event_name,
            "last_dispatch_ms": st.last_dispatch_ms,
            "last_status": st.last_status,
            "webhook_url": st.manifest.webhook_url,
        })
    }
}

// ---------------------------------------------------------------------------
// Central Plugin Registry
// ---------------------------------------------------------------------------

pub struct PluginRegistry {
    plugins: RwLock<Vec<Arc<dyn BossPlugin>>>,
}

impl PluginRegistry {
    pub fn new_with_builtins() -> Self {
        let list: Vec<Arc<dyn BossPlugin>> = vec![
            Arc::new(PosPaymentsPlugin::new()),
            Arc::new(VenueLightingPlugin::new()),
            Arc::new(TournamentEnginePlugin::new()),
            Arc::new(RfidPeripheralsPlugin::new()),
            Arc::new(AlertingSinksPlugin::new()),
        ];
        Self {
            plugins: RwLock::new(list),
        }
    }

    /// Load external `*.json` plugin manifests from `dir` if present.
    pub async fn load_external_manifests(&self, dir: &Path) {
        let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
            return;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if let Ok(raw) = tokio::fs::read_to_string(&path).await {
                if let Ok(manifest) = serde_json::from_str::<PluginManifest>(&raw) {
                    tracing::info!(plugin_id = %manifest.id, "Loaded external plugin manifest");
                    self.register_webhook_plugin(manifest).await;
                }
            }
        }
    }

    pub async fn register_webhook_plugin(&self, manifest: PluginManifest) {
        let mut list = self.plugins.write().await;
        list.push(Arc::new(WebhookPlugin::new(manifest)));
    }

    pub async fn list_descriptors(&self) -> Vec<PluginDescriptor> {
        let list = self.plugins.read().await.clone();
        let mut out = Vec::with_capacity(list.len());
        let now = now_epoch_ms();
        for p in list {
            out.push(PluginDescriptor {
                manifest: p.manifest().await,
                state: p.state_snapshot().await,
                updated_at_epoch_ms: now,
            });
        }
        out
    }

    pub async fn find_by_id(&self, plugin_id: &str) -> Option<Arc<dyn BossPlugin>> {
        let list = self.plugins.read().await.clone();
        for p in list {
            if p.manifest().await.id == plugin_id {
                return Some(p);
            }
        }
        None
    }

    /// Dispatch a `PluginEvent` to all enabled plugins and broadcast a plugin
    /// state update frame on `SseChannel::Plugins`.
    pub async fn dispatch_event(&self, event: PluginEvent, app_state: &SharedState) {
        let list = self.plugins.read().await.clone();
        let ev_name = event.event_name();
        for plugin in list {
            let manifest = plugin.manifest().await;
            if !manifest.enabled {
                continue;
            }
            if manifest
                .subscribed_events
                .iter()
                .any(|s| s == ev_name || s == "*")
            {
                plugin.on_event(&event, app_state).await;
            }
        }

        // For non-heartbeat events, publish an updated plugin state summary to SSE subscribers.
        if !matches!(event, PluginEvent::TelemetryHeartbeat(_)) {
            let descriptors = self.list_descriptors().await;
            app_state.publish(
                SseChannel::Plugins,
                &json!({
                    "kind": "plugin_event_processed",
                    "trigger": ev_name,
                    "plugins": descriptors,
                    "at_epoch_ms": now_epoch_ms(),
                }),
            );
        }
    }
}
