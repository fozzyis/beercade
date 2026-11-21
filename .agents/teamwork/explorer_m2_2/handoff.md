# Handoff Report: B.O.S.S. Command Plane (Requirement R6)

**Agent**: Explorer M2-2  
**Milestone**: Milestone 2 (B.O.S.S. Server Security & Command Plane)  
**Task Focus**: Investigation of B.O.S.S. Command Plane queue, polling, ACK behavior, data models, state structures, and test coverage  
**Date**: 2026-10-27  

---

## 1. Observation

### 1.1 Command Enqueuing
- In `boss/src/routes.rs:457-575`, commands are enqueued via four RPC routes and internal plugins, all delegating to `state.enqueue_command(&node_id, command)`:
  - `POST /api/v1/cabinet/:node_id/reboot` (`routes::cabinet_reboot`, lines 457–473)
  - `POST /api/v1/cabinet/:node_id/ping` (`routes::cabinet_ping`, lines 476–487)
  - `POST /api/v1/cabinet/:node_id/launch_game` (`routes::cabinet_launch_game`, lines 490–517)
  - `POST /api/v1/credits/add` (`routes::credits_add`, lines 521–575)
- In `boss/src/state.rs:255-283`:
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

### 1.2 Command Polling
- In `boss/src/routes.rs:595-617`:
  ```rust
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
- Polling route is bound in `boss/src/main.rs:175`: `.route("/api/v1/cabinet/:node_id/commands", get(routes::poll_commands))`.

### 1.3 Command Acknowledging
- In `boss/src/routes.rs:620-650`:
  ```rust
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
- ACK route is bound in `boss/src/main.rs:176`: `.route("/api/v1/cabinet/:node_id/commands/ack", post(routes::ack_command))`.

### 1.4 Data Models
- In `shared/crates/beercade-shared/src/models.rs:149-210`:
  - `CabinetCommand`: enum with variants `Reboot { delay_secs }`, `LaunchGame { core, rom, netplay }`, `AddCredits { credits }`, `Ping { seq }`, `StartSession { session_id, duration_secs }`, `EndSession`. Serde tag: `type`, `rename_all = "snake_case"`.
  - `CommandStatus`: enum with variants `Queued`, `Dispatched`, `Acknowledged`, `Failed`. Serde `rename_all = "snake_case"`.
  - `CommandRecord`: struct with `id: String`, `node_id: String`, `command: CabinetCommand`, `status: CommandStatus`, `issued_at_epoch_ms: u64`, `dispatched_at_epoch_ms: Option<u64>`, `completed_at_epoch_ms: Option<u64>`, `result: Option<serde_json::Value>`.
  - `CommandAck`: struct with `command_id: String`, `success: bool`, `detail: Option<serde_json::Value>`.

### 1.5 State Structures
- In `boss/src/state.rs:149-151`:
  - `commands: RwLock<HashMap<String, CommandRecord>>`: process-lifetime map of all commands indexed by UUID.
  - `pending_commands: RwLock<HashMap<String, VecDeque<String>>>`: map from `node_id` to FIFO queue of pending command UUIDs.

### 1.6 Existing Test Coverage
- Running `cargo test -p boss-server -- --list`:
  - Found exactly 7 tests: 1 in `auth.rs`, 1 in `ota_sync.rs`, 2 in `netplay_relay.rs`, 3 in `plugins.rs`.
  - Directory `boss/tests` does not exist.
  - No tests exist for command enqueuing, polling, ACKs, or command status in `boss/src/routes.rs` or `boss/src/state.rs`.

---

## 2. Logic Chain

1. **Enqueuing Behavior**: 
   - Based on Section 1.1, calling any of the RPC endpoints or `AppState::enqueue_command` creates a `CommandRecord` with a fresh UUIDv4, timestamps it with `issued_at_epoch_ms`, appends the UUID to the node's `VecDeque` in `pending_commands`, and stores the full record in `commands`.
   - Therefore, commands are queued in strict FIFO order per node and are immediately indexable by command ID.

2. **Polling Behavior**:
   - Based on Section 1.2, `routes::poll_commands` calls `pending.remove(&node_id)`.
   - If no commands are queued, `pending.remove` returns `None`, resulting in an empty list and a 200 OK response with `commands: []`.
   - If commands are queued, the entire `VecDeque` is drained atomically. Each command in `commands` has its `status` updated to `CommandStatus::Dispatched` and `dispatched_at_epoch_ms` populated.
   - Therefore, polling is destructive (drain) and idempotent in HTTP status (subsequent poll returns 200 OK with `[]`).

3. **ACK Ingestion Behavior**:
   - Based on Section 1.3, `routes::ack_command` looks up the command in `state.commands`.
   - If the ID does not exist, it immediately returns HTTP 404.
   - If the command's `node_id` does not match the URL path's `:node_id`, it immediately returns HTTP 403 Forbidden without modifying state.
   - If the node matches, it updates status (`Acknowledged` if `ack.success` is true, `Failed` if false), records completion time and optional detail, and notifies SSE and plugins.
   - Therefore, cross-node ACK forgery is prevented, and command terminal states are captured.

4. **Test Gap Identification**:
   - Based on Section 1.6, there are 0 existing tests covering the command plane in `boss-server`.
   - Therefore, to satisfy Requirement R6 in Milestone 2, unit and/or integration tests must be written to cover all stages of the command plane lifecycle and edge cases.

---

## 3. Caveats

1. **Lack of Timeout / Redelivery for Dispatched Commands**: Because `poll_commands` drains the `pending_commands` queue, if a node crashes or loses power after polling but before ACKing, the command remains in `Dispatched` state indefinitely. There is currently no server-side retry or heartbeat-timeout mechanism for unacknowledged commands.
2. **Duplicate ACK Side Effects**: If a node sends duplicate ACKs for the same command, the second ACK returns 200 OK, but updates `completed_at_epoch_ms` and fires SSE and plugin events again.
3. **Out-of-Order Poll Overwrite**: If an ACK is received for a command that was not yet polled (e.g. out-of-band), its status becomes `Acknowledged`. A subsequent call to `poll_commands` would overwrite the status back to `Dispatched`.
4. **Scope Constraint**: As an explorer, no modifications to source code or tests were made in this task.

---

## 4. Conclusion

The B.O.S.S. command plane implementation in `boss/src/routes.rs` and `boss/src/state.rs` is architecturally clean, robust against cross-node tampering (via explicit 403 checks), and adheres to strict FIFO sequence ordering per node. However, it lacks any automated test coverage. 

For Milestone 2 implementers, comprehensive unit/integration tests should be constructed covering:
1. `enqueue_command` and HTTP RPC routes (`reboot`, `ping`, `launch_game`, `credits/add`).
2. Polling with empty queue (200 with `[]`) and non-empty queue (FIFO ordering, status -> `Dispatched`).
3. Queue draining (second poll returns `[]`).
4. Successful ACK (`Acknowledged`) and failed ACK (`Failed`) with detail payloads.
5. Error conditions: unknown command ID (404), mismatched node ID (403), unauthorized calls (401).
6. Command status query (`/api/v1/commands/:command_id`).

---

## 5. Verification Method

To verify these findings independently:

1. **Verify Existing Tests**:
   ```powershell
   cargo test -p boss-server -- --list
   ```
   Assert that only 7 tests exist and none touch routes or command state.

2. **Inspect Route Implementations**:
   - Review `boss/src/routes.rs:457-650` for `cabinet_reboot`, `cabinet_ping`, `cabinet_launch_game`, `credits_add`, `command_status`, `poll_commands`, and `ack_command`.
   - Review `boss/src/state.rs:255-283` for `enqueue_command`.
   - Review `boss/src/main.rs:168-176` for route registrations.

3. **Inspect Models**:
   - Review `shared/crates/beercade-shared/src/models.rs:149-210` for `CabinetCommand`, `CommandStatus`, `CommandRecord`, and `CommandAck`.
