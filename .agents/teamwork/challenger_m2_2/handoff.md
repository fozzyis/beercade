# Handoff Report: Milestone 2 Adversarial Challenge — Command Plane Queue, Polling & ACK (R6)

**Agent**: Challenger M2-2 (`challenger_m2_2`)  
**Role**: Empirical Challenger (critic, specialist)  
**Date**: 2026-10-27T23:34:00Z  
**Recipient**: Orchestrator (`bd770878-29b7-48ec-a1e1-b5265681a707`)  
**Milestone**: Milestone 2 (Command Plane Verification R6)  
**Verdict**: `APPROVE`

---

## 1. Observation

1. **Source Inspection in `boss/src/routes.rs`**:
   - Lines 680–702 (`poll_commands`):
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
   - Lines 705–735 (`ack_command`):
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
   - Lines 663–673 (`command_status`):
     ```rust
     pub async fn command_status(
         State(state): State<SharedState>,
         _auth: RequireRpcToken,
         Path(command_id): Path<String>,
     ) -> Json<serde_json::Value> {
         let commands = state.commands.read().await;
         match commands.get(&command_id) {
             Some(record) => Json(serde_json::to_value(record).unwrap_or_else(|_| json!({}))),
             None => Json(json!({ "error": "not_found", "command_id": command_id })),
         }
     }
     ```

2. **Source Inspection in `boss/src/state.rs`**:
   - Lines 385–412 (`enqueue_command`):
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
         ...
     ```

3. **Built-in Cargo Test Execution**:
   - Executing `cargo test -p boss-server -- routes::tests::test_f6`:
     ```text
     running 8 tests
     test routes::tests::test_f6_command_ack_unknown_command_returns_404 ... ok
     test routes::tests::test_f6_command_endpoints_require_auth ... ok
     test routes::tests::test_f6_command_ack_mismatched_node_returns_403 ... ok
     test routes::tests::test_f6_command_polling_drains_queue_second_poll_empty ... ok
     test routes::tests::test_f6_command_queue_isolation_between_nodes ... ok
     test routes::tests::test_f6_command_ack_success ... ok
     test routes::tests::test_f6_command_ack_failure ... ok
     test routes::tests::test_f6_command_enqueuing_and_fifo_polling ... ok

     test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 20 filtered out; finished in 0.01s
     ```

4. **Empirical Live Server Adversarial Challenge Harness**:
   - Launched `target\debug\boss-server.exe` on port 19890 with separate `BOSS_RPC_TOKEN=adv_rpc_secret_123` and `BOSS_NODE_TOKEN=adv_node_secret_456`.
   - Executed 10 live HTTP test vectors covering edge cases:
     ```text
     [OK] Pass 1: Empty queue returns 200 with empty list
     [OK] Pass 2: Polling drains the queue and 2nd poll returns empty
     [OK] Pass 3: Command preserved in commands map in Dispatched status with timestamps
     [OK] Pass 4: ACK updates status to Acknowledged (on success) or Failed (on error) with payload
     [OK] Pass 5: ACK on unknown command ID returns 404
     [OK] Pass 6: ACK on mismatched node ID returns 403 Forbidden and state is untouched
     [OK] Pass 7: Strict queue isolation between distinct node IDs
     [OK] Pass 8: Unauthenticated / bad token polling and ACK return 401
     [OK] Pass 9: Burst test: 15 rapid commands polled with 100% strict FIFO order
     [OK] Pass 10: Privilege separation enforced (Node token rejected on RPC endpoints)
     ALL 10 EMPIRICAL ADVERSARIAL CHALLENGES PASSED PERFECTLY
     ```

5. **Full Workspace Regression Verification**:
   - `cargo test --workspace` result:
     - `astro_daemon`: 10 passed; 0 failed
     - `astro_tune`: 5 passed; 0 failed
     - `beercade_hal`: 9 passed; 0 failed
     - `beercade_shared`: 11 passed; 0 failed
     - `boss_server`: 28 passed; 0 failed
     - `Doc-tests`: 0 passed; 0 failed
     - Total: 63 passed; 0 failed across entire workspace.

---

## 2. Logic Chain

1. **Empty Queue Polling**:
   - Observation 1 shows `pending.remove(&node_id).unwrap_or_default()`. If a node has never had commands enqueued, `remove` returns `None`, defaulting to an empty `VecDeque`.
   - Observation 4 (Pass 1) confirms empirically via live HTTP that querying a brand-new node (`fresh-never-seen-node`) returns HTTP 200 with `{"node_id": "fresh-never-seen-node", "commands": []}`.

2. **Queue Drain & Idempotence**:
   - Observation 1 shows `pending.remove(&node_id)` completely takes the queue out of the `pending_commands` map.
   - Observation 3 (`test_f6_command_polling_drains_queue_second_poll_empty`) and Observation 4 (Pass 2) confirm that the first poll retrieves the queued command, and an immediate subsequent poll yields an empty command list.

3. **Status Preservation in Memory**:
   - Observation 1 and 2 show that commands are stored in `state.commands` by UUID upon enqueueing with status `Queued`.
   - When drained by `poll_commands`, each record in `state.commands` is mutated to `Dispatched` and assigned `dispatched_at_epoch_ms = Some(now)`.
   - Observation 4 (Pass 3) confirms that querying `/api/v1/commands/{command_id}` returns the record in `Dispatched` status with non-zero `issued_at_epoch_ms` and `dispatched_at_epoch_ms`, while `completed_at_epoch_ms` and `result` remain `null`.

4. **ACK State Transitions & Result Capture**:
   - Observation 1 shows `ack_command` inspects `ack.success`: sets status to `Acknowledged` if true, or `Failed` if false, and populates `completed_at_epoch_ms` and `result = ack.detail`.
   - Observation 3 (`test_f6_command_ack_success`, `test_f6_command_ack_failure`) and Observation 4 (Pass 4) verify both branches empirically, confirming the exact JSON detail payload is persisted.

5. **Unknown Command ID Handling**:
   - Observation 1 shows `commands.get_mut(&ack.command_id)` returning `None` emits `StatusCode::NOT_FOUND` (404).
   - Observation 3 (`test_f6_command_ack_unknown_command_returns_404`) and Observation 4 (Pass 5) verify that random or zeroed UUIDs return 404 Not Found.

6. **Mismatched Node ID Security (Cross-Cabinet Impersonation)**:
   - Observation 1 shows `if record.node_id != node_id { return StatusCode::FORBIDDEN; }`.
   - Observation 3 (`test_f6_command_ack_mismatched_node_returns_403`) and Observation 4 (Pass 6) confirm that when `cab-impostor` attempts to ACK a command issued to `cab-target`, B.O.S.S. rejects the request with HTTP 403 Forbidden and the victim cabinet's command remains in its uncompromised `dispatched` state.

7. **Queue Isolation Between Cabinets**:
   - Observation 2 shows `pending_commands` partitions queued command UUIDs by `node_id`.
   - Observation 3 (`test_f6_command_queue_isolation_between_nodes`) and Observation 4 (Pass 7) confirm that commands enqueued for Node A are never visible to Node B.

8. **Authentication & Privilege Separation**:
   - Observation 1 and `boss/src/auth.rs` enforce `RequireNodeToken` on polling/ACK and `RequireRpcToken` on enqueueing.
   - Observation 3 (`test_f6_command_endpoints_require_auth`) and Observation 4 (Pass 8, Pass 10) confirm that unauthenticated requests receive HTTP 401, and node tokens cannot queue RPC commands.

9. **Burst Queueing & FIFO Integrity**:
   - Observation 2 uses `VecDeque::push_back` and `poll_commands` drains in forward iteration order.
   - Observation 4 (Pass 9) empirically confirms that 15 rapid consecutive command dispatches preserve 100% strict FIFO sequence.

---

## 3. Caveats

1. **In-Memory Retention**: Command queues and records are retained in-memory within `AppState` for the lifetime of the B.O.S.S. daemon process. If the server is restarted, unpolled commands and historical ACK records are reset (as designed per `boss/SPECIFICATION.md`).
2. **Double-ACK Behavior**: If a cabinet ACKs an already acknowledged command, the handler will overwrite the result and timestamp. In the existing protocol, nodes ACK commands only once upon execution completion.
3. **No Image Builds**: In strict adherence to project constraints, no disk images, rootfs exports, or container builds were executed.

---

## 4. Conclusion

**Verdict: `APPROVE`**

The implementation of Requirement R6 (Command Plane Queue, Polling, and ACK mechanics) in `boss-server` is robust, secure, and adheres strictly to the architectural specifications:
- Empty polling returns HTTP 200 with an empty list.
- Polling atomically drains the pending queue.
- Dispatched commands remain queryable with timestamps.
- ACKs correctly transition status to `Acknowledged` or `Failed` with result payloads.
- Unknown command IDs return 404; cross-cabinet ACK spoofing is rejected with 403.
- Per-node queues are strictly isolated.
- Authentication and privilege boundaries are enforced.
- All 8 co-located unit tests and 10 live adversarial challenges pass with 0 errors.

---

## 5. Verification Method

To independently verify this evaluation:

1. **Run Built-in Route Tests for F6**:
   ```powershell
   cargo test -p boss-server -- routes::tests::test_f6
   ```
   **Expected**: 8 passed; 0 failed.

2. **Run Full B.O.S.S. Test Suite**:
   ```powershell
   cargo test -p boss-server
   ```
   **Expected**: 28 passed; 0 failed.

3. **Run Full Workspace Tests**:
   ```powershell
   cargo test --workspace
   ```
   **Expected**: 63 passed; 0 failed.

4. **Invalidation Conditions**:
   - Polling an empty queue returns non-200 or null.
   - Polling a queue twice returns the same command twice (failure to drain).
   - An ACK with `record.node_id != node_id` returns 200 instead of 403.
   - An ACK with unknown command ID returns 200 instead of 404.
   - Any test in `cargo test -p boss-server -- routes::tests::test_f6` fails.
