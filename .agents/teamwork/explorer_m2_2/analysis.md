# B.O.S.S. Command Plane Technical Analysis (Requirement R6)

**Explorer**: M2-2  
**Date**: 2026-10-27  
**Scope**: B.O.S.S. Server Command Plane Queue, Polling, ACK Ingestion, Data Models, State Structures, and Test Gap Analysis  
**Repository**: `d:\data\dev\beercade`

---

## 1. Executive Summary

This report delivers a deep technical investigation into the B.O.S.S. (B.E.E.R.C.A.D.E. Operations & Supervisory System) Command Plane subsystem, satisfying Requirement R6 of Milestone 2. 

The command plane provides asynchronous supervisory control over remote A.S.T.R.O. cabinet stations. It is designed around an out-of-band pull architecture: supervisory clients (kiosk UI, operator admin, cloud edge, internal plugins) enqueue commands destined for specific arcade nodes; nodes periodically poll B.O.S.S. over HTTP to drain their queued commands, execute them locally, and return execution acknowledgements (ACKs) with detailed outcome telemetry.

Key findings:
1. **Enqueuing Endpoints**: B.O.S.S. does not expose a single generic `POST /api/v1/nodes/:id/commands` route; instead, it provides four specialized RPC routes:
   - `POST /api/v1/cabinet/:node_id/reboot`
   - `POST /api/v1/cabinet/:node_id/ping`
   - `POST /api/v1/cabinet/:node_id/launch_game`
   - `POST /api/v1/credits/add` (with `node_id` in JSON body)
   All endpoints enforce `RequireRpcToken` and delegate to `AppState::enqueue_command`.
2. **Polling Endpoint**: `GET /api/v1/cabinet/:node_id/commands` enforces `RequireNodeToken`. It is a destructive drain: it atomically removes all pending command IDs for that node from `state.pending_commands`, updates their lifecycle state in `state.commands` to `Dispatched` with `dispatched_at_epoch_ms = Some(now)`, and returns the records in FIFO order.
3. **ACK Endpoint**: `POST /api/v1/cabinet/:node_id/commands/ack` receives a `CommandAck` payload, verifies node ownership (returning `403 Forbidden` on mismatch, `404 Not Found` if missing), updates lifecycle status to `Acknowledged` or `Failed`, records `completed_at_epoch_ms` and `result`, and emits events onto SSE `SseChannel::Commands` and `PluginEvent::CommandUpdated`.
4. **State Structures**: In-memory state uses `pending_commands: RwLock<HashMap<String, VecDeque<String>>>` for FIFO queuing and `commands: RwLock<HashMap<String, CommandRecord>>` for boot-scoped lifecycle indexing.
5. **Test Coverage Gap**: Currently, **zero tests** exist for command enqueuing, polling, ACK handling, or status checking in `boss/src` or `boss/tests`. Complete test infrastructure must be added in Milestone 2.

---

## 2. Architecture & Data Flow

```
+-----------------------------------------------------------------------------------------+
|                                    B.O.S.S. Server                                      |
|                                                                                         |
|  1. Ingress RPC:                                                                        |
|     POST /api/v1/cabinet/:node_id/{reboot, ping, launch_game}  [RequireRpcToken]        |
|     POST /api/v1/credits/add                                    [RequireRpcToken]        |
|          |                                                                              |
|          v                                                                              |
|  2. State Queuing (AppState::enqueue_command):                                          |
|     - Generate UUIDv4 id                                                                |
|     - Insert CommandRecord into commands (HashMap<String, CommandRecord>)                |
|     - Push ID to pending_commands[node_id] (VecDeque<String>) (FIFO)                    |
|     - Broadcast to SSE: SseChannel::Commands                                            |
|                                                                                         |
|  3. Node Poll:                                                                          |
|     GET /api/v1/cabinet/:node_id/commands                       [RequireNodeToken]       |
|     - Drain pending_commands.remove(node_id)                                            |
|     - Transition CommandRecord.status: Queued -> Dispatched                             |
|     - Set dispatched_at_epoch_ms = Some(now)                                            |
|     - Return {"node_id": "...", "commands": [...]}                                      |
|                                                                                         |
|  4. Node ACK Ingestion:                                                                 |
|     POST /api/v1/cabinet/:node_id/commands/ack                  [RequireNodeToken]       |
|     - Verify node_id == record.node_id (403 if mismatch, 404 if not found)              |
|     - Transition status: Dispatched -> Acknowledged / Failed                            |
|     - Set completed_at_epoch_ms = Some(now), record.result = ack.detail                 |
|     - Broadcast to SSE: SseChannel::Commands & dispatch PluginEvent::CommandUpdated     |
|                                                                                         |
|  5. Command Status Query:                                                               |
|     GET /api/v1/commands/:command_id                            [RequireRpcToken]        |
|     - Look up record in state.commands                                                  |
+-----------------------------------------------------------------------------------------+
                                         ^         |
                         HTTP Bearer     |         | HTTP Bearer
                         Node Token      |         | Node Token
                                         |         v
+-----------------------------------------------------------------------------------------+
|                                  A.S.T.R.O. Daemon                                      |
|  - Polling loop (every 2s): GET /api/v1/cabinet/{node_id}/commands                       |
|  - Dispatcher: execute_cabinet_command(&cmd, &state, &client)                           |
|  - Outcome reporter: POST /api/v1/cabinet/{node_id}/commands/ack                        |
+-----------------------------------------------------------------------------------------+
```

---

## 3. Command Enqueuing Deep Dive

### 3.1 Routing & Handlers (`boss/src/routes.rs`, `boss/src/main.rs`)

Commands enter B.O.S.S. through authenticated RPC handlers defined in `boss/src/routes.rs` and bound in `boss/src/main.rs`:

```rust
// boss/src/main.rs:168-176
.route("/api/v1/cabinet/:node_id/reboot", post(routes::cabinet_reboot))
.route("/api/v1/cabinet/:node_id/ping", post(routes::cabinet_ping))
.route("/api/v1/cabinet/:node_id/launch_game", post(routes::cabinet_launch_game))
.route("/api/v1/credits/add", post(routes::credits_add))
.route("/api/v1/commands/:command_id", get(routes::command_status))
```

#### Detailed Handler Specifications:

| Endpoint | Method | Auth Guard | Request Body | Enqueued Command Variant | Response (200 OK) |
|---|---|---|---|---|---|
| `/api/v1/cabinet/:node_id/reboot` | `POST` | `RequireRpcToken` | `Option<Json<RebootRequest>>` (`delay_secs`: optional u32, default 5, capped at 600) | `CabinetCommand::Reboot { delay_secs }` | `{"command_id": "...", "node_id": "...", "status": "queued", "issued_at_epoch_ms": ...}` |
| `/api/v1/cabinet/:node_id/ping` | `POST` | `RequireRpcToken` | None | `CabinetCommand::Ping { seq: now_epoch_ms() }` | `{"command_id": "...", "node_id": "...", "status": "queued", "issued_at_epoch_ms": ...}` |
| `/api/v1/cabinet/:node_id/launch_game` | `POST` | `RequireRpcToken` | `Json<LaunchGameRequest>` (`core`: String, `rom`: String, `netplay`: bool) | `CabinetCommand::LaunchGame { core, rom, netplay }` | `{"command_id": "...", "node_id": "...", "status": "queued", "issued_at_epoch_ms": ...}` |
| `/api/v1/credits/add` | `POST` | `RequireRpcToken` | `Json<AddCreditsRequest>` (`node_id`: String, `credits`: u32, 1..=100) | `CabinetCommand::AddCredits { credits }` | `{"command_id": "...", "node_id": "...", "credits": n, "projected_balance": m, "status": "queued", ...}` |

#### Plugin Enqueuing:
Plugins running inside B.O.S.S. (e.g., `pos-payments` in `boss/src/plugins.rs`: lines 538–551) can directly enqueue commands:
- `CabinetCommand::StartSession { session_id, duration_secs }`
- `CabinetCommand::EndSession`
- `CabinetCommand::AddCredits { credits }`

### 3.2 State Enqueue Logic (`boss/src/state.rs:255-283`)

```rust
pub async fn enqueue_command(
    &self,
    node_id: &str,
    command: crate::models::CabinetCommand,
) -> CommandRecord {
    let record = CommandRecord {
        id: uuid::Uuid::new_v4().to_string(),
        node_id: node_id.to_string(),
        command,
        status: CommandStatus::Queued,
        issued_at_epoch_ms: now_epoch_ms(),
        dispatched_at_epoch_ms: None,
        completed_at_epoch_ms: None,
        result: None,
    };

    self.pending_commands
        .write()
        .await
        .entry(node_id.to_string())
        .or_default()
        .push_back(record.id.clone());
    self.commands
        .write()
        .await
        .insert(record.id.clone(), record.clone());
    self.publish(SseChannel::Commands, &record);
    record
}
```

**Key Invariants:**
- Every command is assigned an immutable UUIDv4 string ID.
- The command ID is added to the tail (`push_back`) of the `VecDeque` associated with `node_id`, ensuring strict FIFO ordering.
- A clone of the `CommandRecord` is stored in `self.commands` (hash map index).
- An SSE event is published immediately to `SseChannel::Commands`.

---

## 4. Command Polling Deep Dive

### 4.1 Endpoint & Handler (`boss/src/routes.rs:594-617`)

```rust
/// GET /api/v1/cabinet/:node_id/commands — drain this node's pending commands.
pub async fn poll_commands(
    State(state): State<SharedState>,
    RequireNodeToken(claim): RequireNodeToken,
    Path(node_id): Path<String>,
) -> Json<serde_json::Value> {
    let _ = claim;
    let mut pending = state.pending_commands.write().await;
    let ids = pending.remove(&node_id).unwrap_or_default();
    drop(pending);

    let mut commands_map = state.commands.write().await;
    let now = now_epoch_ms();
    let mut dispatched: Vec<CommandRecord> = Vec::with_capacity(ids.len());
    for id in &ids {
        if let Some(record) = commands_map.get_mut(id) {
            record.status = CommandStatus::Dispatched;
            record.dispatched_at_epoch_ms = Some(now);
            dispatched.push(record.clone());
        }
    }

    Json(json!({ "node_id": node_id, "commands": dispatched }))
}
```

### 4.2 Polling Mechanics & Guarantees
1. **Destructive Drain**: `pending.remove(&node_id)` completely removes the node's entry from `pending_commands`. It does not keep unacknowledged items in the pending queue.
2. **Atomic Status Mutation**: Commands are transitioned from `Queued` to `Dispatched`, and `dispatched_at_epoch_ms` is stamped with `now_epoch_ms()`.
3. **Sequence Preservation**: The order of `ids` in the `VecDeque` is preserved in the resulting `commands` JSON array.
4. **Client-Side Poller**: As observed in `astro/daemon/src/main.rs:815-883`, the daemon polls this endpoint every 2 seconds:
   - Sets `Authorization: Bearer <boss_token>`
   - Iterates over returned array: for each command, invokes `execute_cabinet_command()` and immediately sends ACK.

---

## 5. Command Acknowledging Deep Dive

### 5.1 Endpoint & Handler (`boss/src/routes.rs:620-650`)

```rust
/// POST /api/v1/cabinet/:node_id/commands/ack — node reports command outcome.
pub async fn ack_command(
    State(state): State<SharedState>,
    RequireNodeToken(_claim): RequireNodeToken,
    Path(node_id): Path<String>,
    Json(ack): Json<CommandAck>,
) -> StatusCode {
    let now = now_epoch_ms();
    let mut commands = state.commands.write().await;
    if let Some(record) = commands.get_mut(&ack.command_id) {
        if record.node_id != node_id {
            return StatusCode::FORBIDDEN;
        }
        record.status = if ack.success {
            CommandStatus::Acknowledged
        } else {
            CommandStatus::Failed
        };
        record.completed_at_epoch_ms = Some(now);
        record.result = ack.detail.clone();
        let snapshot = record.clone();
        drop(commands);
        state.publish(SseChannel::Commands, &snapshot);
        state
            .plugins
            .dispatch_event(PluginEvent::CommandUpdated(snapshot), &state)
            .await;
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}
```

### 5.2 ACK Validation Rules & Lifecycle Transitions

1. **Existence Check**:
   - `commands.get_mut(&ack.command_id)`
   - If ID is unknown: returns HTTP `404 Not Found`.
2. **Node Ownership Verification**:
   - `if record.node_id != node_id { return StatusCode::FORBIDDEN; }`
   - If a cabinet attempts to ACK a command issued to another cabinet, returns HTTP `403 Forbidden`.
   - The record remains untouched.
3. **Outcome Resolution**:
   - `ack.success == true` -> `CommandStatus::Acknowledged`
   - `ack.success == false` -> `CommandStatus::Failed`
4. **Completion Timestamp & Result Capture**:
   - `record.completed_at_epoch_ms = Some(now)`
   - `record.result = ack.detail.clone()`
5. **Event Bus Propagation**:
   - Published to `SseChannel::Commands` for live UI dashboards.
   - Dispatched to plugin bus via `PluginEvent::CommandUpdated(snapshot)` for audit/metrics logging.

---

## 6. Data Models (`beercade-shared::models`)

The data contracts are declared in `shared/crates/beercade-shared/src/models.rs` and re-exported in `boss/src/models.rs`:

```rust
/// Operator- or cloud-initiated command destined for a cabinet node.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CabinetCommand {
    Reboot {
        delay_secs: u32,
    },
    LaunchGame {
        core: String,
        rom: String,
        netplay: bool,
    },
    AddCredits {
        credits: u32,
    },
    Ping {
        seq: u64,
    },
    StartSession {
        session_id: String,
        duration_secs: u32,
    },
    EndSession,
}

/// Lifecycle state of a dispatched command.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    Queued,
    Dispatched,
    Acknowledged,
    Failed,
}

/// A command tracked from issuance to acknowledgement.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CommandRecord {
    pub id: String,
    pub node_id: String,
    pub command: CabinetCommand,
    pub status: CommandStatus,
    pub issued_at_epoch_ms: u64,
    #[serde(default)]
    pub dispatched_at_epoch_ms: Option<u64>,
    #[serde(default)]
    pub completed_at_epoch_ms: Option<u64>,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
}

/// Node-poll payload acknowledging a dispatched command.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CommandAck {
    pub command_id: String,
    pub success: bool,
    #[serde(default)]
    pub detail: Option<serde_json::Value>,
}
```

### JSON Wire Formats

#### `CabinetCommand` Wire Representations:
- **Ping**: `{"type": "ping", "seq": 1729900000000}`
- **AddCredits**: `{"type": "add_credits", "credits": 4}`
- **LaunchGame**: `{"type": "launch_game", "core": "fbneo", "rom": "sfiii3.zip", "netplay": false}`
- **Reboot**: `{"type": "reboot", "delay_secs": 5}`
- **StartSession**: `{"type": "start_session", "session_id": "sess_9a8b", "duration_secs": 1800}`
- **EndSession**: `{"type": "end_session"}`

#### `CommandAck` Wire Representation:
```json
{
  "command_id": "93bfa3cb-e89c-4573-8994-0a373b5df511",
  "success": true,
  "detail": {
    "credits_added": 4,
    "credits_available": 8
  }
}
```

---

## 7. State Structures & Concurrency in `boss/src/state.rs`

In `boss/src/state.rs`:

```rust
pub struct AppState {
    pub config: Config,
    ...
    /// All commands ever issued this boot, by id.
    pub commands: RwLock<HashMap<String, CommandRecord>>,
    /// Pending (not yet polled) command ids per node, FIFO order.
    pub pending_commands: RwLock<HashMap<String, VecDeque<String>>>,
    ...
}
```

### Concurrency Characteristics:
1. **Lock Separation**: `pending_commands` and `commands` have separate `tokio::sync::RwLock`s.
   - During `enqueue_command`:
     - Acquires `pending_commands.write().await`, pushes ID, drops.
     - Acquires `commands.write().await`, inserts record, drops.
   - During `poll_commands`:
     - Acquires `pending_commands.write().await`, removes `node_id`, drops.
     - Acquires `commands.write().await`, updates statuses, drops.
   - During `ack_command`:
     - Only acquires `commands.write().await`.
2. **Lock Order & Deadlock Safety**: In all call sites, locks are acquired in the order `pending_commands` -> `commands`, and the `pending_commands` lock is dropped before acquiring `commands`. There are zero nested/interleaved lock acquisitions, preventing deadlocks.
3. **Memory Footprint**: `commands` retains every command issued during the process lifecycle in RAM. For high-volume environments, an unbounded HashMap can grow, but for cabinet fleets in venue operations, command volume is small (<10k per day).

---

## 8. Edge Cases, Failure Modes & Vulnerabilities

| # | Edge Case | Observed Behavior in Code | Risk / Impact | Status |
|---|---|---|---|---|
| **E1** | **Polling with empty queue** | `pending.remove(&node_id)` returns `None`. Handled with `unwrap_or_default()`. Returns HTTP 200 with `{"node_id": node_id, "commands": []}`. | None. Safe and idempotent. | Verified Safe |
| **E2** | **Duplicate Poll (Double Polling)** | First poll drains all IDs. Second poll immediately returns `[]`. | None. Handled properly. | Verified Safe |
| **E3** | **ACK with invalid command ID** | `commands.get_mut` returns `None`. Route returns `StatusCode::NOT_FOUND` (404). | None. Correct REST semantics. | Verified Safe |
| **E4** | **Cross-Node ACK mismatch** | `record.node_id != node_id` triggers `return StatusCode::FORBIDDEN` (403). Record is untouched. | None. Prevents cabinet impersonation / cross-ACK. | Verified Safe |
| **E5** | **Duplicate ACK** | Overwrites `status`, `completed_at_epoch_ms`, and `result`, re-publishes to SSE, and returns 200 OK. | Low. Idempotent HTTP return, but causes duplicate event firing and updates completion timestamp. | Handled (with minor side-effect) |
| **E6** | **ACK prior to poll (Out-of-order ACK)** | If a command is ACKed while `Queued`: status becomes `Acknowledged`. When the node later polls, `poll_commands` blindly overwrites `status` back to `Dispatched`! | Medium. Edge case if node out-of-bands an ACK before polling. | Latent edge case |
| **E7** | **Dropped / Unacked Command Timeout** | When a node crashes after polling commands, `pending_commands` has already drained. B.O.S.S. does not retry or expire `Dispatched` commands. | Operational. Requires manual reissue if node reboots mid-flight. | Architectural choice |
| **E8** | **Command Status query 404 behavior** | `GET /api/v1/commands/:command_id` returns HTTP 200 with `{"error": "not_found", "command_id": command_id}` rather than HTTP 404. | Minor. Inconsistent with standard HTTP 404, but client can inspect JSON. | Handled |
| **E9** | **FIFO Sequence Guarantee** | `push_back` on enqueue and sequential draining preserves exact order across multiple commands. | None. Order is strictly preserved. | Verified Safe |
| **E10** | **Authentication Fail-Closed** | Missing or bad Bearer token rejected with 401 Unauthorized by `RequireRpcToken` / `RequireNodeToken`. | High Security. Verified fail-closed. | Verified Safe |

---

## 9. Existing Test Coverage & Gap Analysis

### 9.1 Audit of Existing Tests in `boss-server`
Executing `cargo test -p boss-server -- --list` reveals exactly 7 tests:
- `auth::tests::test_constant_time_eq_delegation`
- `netplay_relay::tests::relay_discovery_and_forwarding`
- `netplay_relay::tests::relay_health_report_ingest`
- `ota_sync::tests::test_sanitize_filename`
- `plugins::tests::test_thermal_alert_plugin_formats_missing_sensor_as_na`
- `plugins::tests::test_thermal_alert_plugin_with_none_thermals`
- `plugins::tests::test_thermal_alert_plugin_with_normal_thermals`

### 9.2 Missing Test Coverage for Command Plane
1. **Queueing tests**: Zero tests for `AppState::enqueue_command` or HTTP handlers (`cabinet_reboot`, `cabinet_ping`, `cabinet_launch_game`, `credits_add`).
2. **Polling tests**: Zero tests for `routes::poll_commands` or `GET /api/v1/cabinet/:node_id/commands`.
3. **ACK tests**: Zero tests for `routes::ack_command` or `POST /api/v1/cabinet/:node_id/commands/ack`.
4. **Status inspection tests**: Zero tests for `routes::command_status`.
5. **Edge case tests**: Zero tests verifying empty poll, cross-node ACK 403, unknown command 404, duplicate ACK, or FIFO multi-command ordering.
6. **Authentication tests**: Zero tests verifying that unauthenticated calls to command endpoints fail with 401.

---

## 10. Recommendations for Milestone 2 Test Implementation

Per `TEST_INFRA.md`, automated command plane tests should cover the complete 4-tier matrix:

### Tier 1: Feature Functionality
- `test_enqueue_and_poll_single_command`: Enqueue a command (e.g. `Ping`), poll, assert status transitions `Queued` -> `Dispatched`, assert payload matches.
- `test_ack_successful_command`: Send success ACK (`success: true`), assert status transitions `Dispatched` -> `Acknowledged`, verify `completed_at_epoch_ms` and `result`.
- `test_ack_failed_command`: Send failure ACK (`success: false`), assert status transitions `Dispatched` -> `Failed` with error detail.
- `test_command_status_query`: Query `/api/v1/commands/:command_id` at each lifecycle stage (`Queued`, `Dispatched`, `Acknowledged`, `Failed`).

### Tier 2: Boundary & Edge Cases
- `test_poll_empty_queue`: Poll when no commands are queued, assert HTTP 200 and `commands: []`.
- `test_poll_drains_queue`: First poll returns items; second poll immediately returns empty array.
- `test_ack_nonexistent_command`: ACK random UUID, assert HTTP 404 Not Found.
- `test_ack_cross_node_mismatch`: Node `CAB-02` attempts to ACK command for `CAB-01`, assert HTTP 403 Forbidden.
- `test_duplicate_ack`: Second ACK returns HTTP 200 OK.
- `test_fifo_command_ordering`: Enqueue `Ping(1)`, `AddCredits(5)`, `Reboot(10)`. Poll and assert exact order `[Ping(1), AddCredits(5), Reboot(10)]`.

### Tier 3: Cross-Feature Integration
- `test_multi_node_queue_isolation`: Enqueue commands for `CAB-01` and `CAB-02`. Poll `CAB-01`, assert only `CAB-01` commands returned; `CAB-02` remains queued.
- `test_credits_add_full_pipeline`: Call `/api/v1/credits/add`, verify command is queued, `TokenTransaction` is recorded in state, and credit balance projected.
- `test_auth_enforcement_on_command_plane`: Verify 401 without token on `/api/v1/cabinet/:node_id/commands`, `/ack`, and RPC routes. Verify 200 with `BOSS_NODE_TOKEN` or `BOSS_RPC_TOKEN`.

### Tier 4: Real-World Scenario
- `test_full_remote_supervisory_command_roundtrip`: Simulates an operator enqueuing an `AddCredits` command via admin RPC, an arcade cabinet polling the command, executing it, returning an ACK with coin counter detail, and the admin querying the command to verify it is marked `Acknowledged`.
