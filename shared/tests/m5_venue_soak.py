#!/usr/bin/env python3
"""M5 Mixed-Arch Venue Soak & Acceptance Harness for B.E.E.R.C.A.D.E.

Modes:
  --mode smoke (default):
      Spawns a live compiled `boss-server` instance plus a local SQLite-backed
      Cloudflare D1/KV `/api/v1/edge/ingest` server and exercises all 4 M5 planes:
        A. Mixed-arch telemetry (`x86_64` + `aarch64`) -> B.O.S.S. -> edge ingest -> D1/KV parity
        B. Continuous `BRC-XXXX` patron session lifecycle (`0` dropped sessions)
        C. UDP `:9750` netplay relay announce, watchdog, and bidirectional packet relay
        D. OTA payload upload & `X-OTA-SHA256` streaming download parity

  --mode venue:
      Runs against a live venue B.O.S.S. server (`--boss-url`) and physical
      `x86_64` + `aarch64` cabinets over `--duration-secs` (default 86400 / 24h),
      checkpointing `out/m5-soak-report.json` periodically.
"""

from __future__ import annotations

import argparse
import hashlib
import http.server
import json
import os
import shutil
import socket
import socketserver
import sqlite3
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any, Optional

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
MIGRATION_0001 = REPO_ROOT / "website" / "migrations" / "0001_telemetry_analytics.sql"

MAGIC = b"OR"
WIRE_VER = 1
MSG_INPUT = 0x01
MSG_WATCHDOG = 0x06
MSG_ANNOUNCE = 0x07
MSG_ANNOUNCE_ACK = 0x08


def free_tcp_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def free_udp_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def find_boss_binary() -> Path:
    candidates: list[Path] = []
    cargo_target = os.environ.get("CARGO_TARGET_DIR")
    if cargo_target:
        ct = Path(cargo_target)
        candidates.extend(
            [
                ct / "release" / "boss-server.exe",
                ct / "release" / "boss-server",
                ct / "debug" / "boss-server.exe",
                ct / "debug" / "boss-server",
            ]
        )
    candidates.extend(
        [
            REPO_ROOT / "target" / "release" / "boss-server.exe",
            REPO_ROOT / "target" / "release" / "boss-server",
            REPO_ROOT / "target" / "debug" / "boss-server.exe",
            REPO_ROOT / "target" / "debug" / "boss-server",
        ]
    )
    for path in candidates:
        if path.is_file():
            return path
    raise FileNotFoundError(
        "boss-server binary not found in target/{release,debug}; run `cargo build -p boss-server --locked` first."
    )


def http_request(
    url: str,
    *,
    method: str = "GET",
    headers: Optional[dict[str, str]] = None,
    json_body: Any = None,
    raw_body: Optional[bytes] = None,
    timeout: float = 10.0,
) -> tuple[int, dict[str, str], bytes]:
    req_headers = dict(headers or {})
    data: Optional[bytes] = raw_body
    if json_body is not None:
        data = json.dumps(json_body).encode("utf-8")
        req_headers.setdefault("Content-Type", "application/json")
    req = urllib.request.Request(url, data=data, headers=req_headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            resp_headers = {k.lower(): v for k, v in resp.headers.items()}
            return resp.status, resp_headers, resp.read()
    except urllib.error.HTTPError as err:
        resp_headers = {k.lower(): v for k, v in (err.headers.items() if err.headers else [])}
        return err.code, resp_headers, err.read()


class EdgeMockStore:
    """Thread-safe SQLite D1 + in-memory KV store mimicking Cloudflare edge ingest."""

    def __init__(self, db_path: Path, expected_token: str) -> None:
        self.expected_token = expected_token
        self.lock = threading.Lock()
        self.conn = sqlite3.connect(str(db_path), check_same_thread=False)
        self.conn.executescript(MIGRATION_0001.read_text(encoding="utf-8"))
        self.conn.commit()
        self.seen_batches: set[str] = set()
        self.latest_venue: dict[str, dict[str, Any]] = {}
        self.latest_node: dict[str, dict[str, Any]] = {}
        self.accepted_batches = 0
        self.deduped_batches = 0
        self.rejected_batches = 0
        self.sample_envelopes: list[dict[str, Any]] = []

    def close(self) -> None:
        with self.lock:
            self.conn.close()

    def ingest(self, token: Optional[str], batch: dict[str, Any]) -> tuple[int, dict[str, Any]]:
        if not token or token != self.expected_token:
            with self.lock:
                self.rejected_batches += 1
            return 401, {"error": "Invalid or missing ingest token"}

        batch_id = batch.get("batch_id")
        venue = batch.get("venue")
        nodes = batch.get("nodes")
        errors = batch.get("errors")
        transactions = batch.get("transactions")
        scores = batch.get("scores")
        generated_at = batch.get("generated_at_epoch_ms")
        if (
            not isinstance(batch_id, str)
            or not batch_id
            or not isinstance(venue, str)
            or not isinstance(generated_at, (int, float))
            or not isinstance(nodes, list)
            or not isinstance(errors, list)
            or not isinstance(transactions, list)
            or not isinstance(scores, list)
        ):
            with self.lock:
                self.rejected_batches += 1
            return 422, {"error": "Envelope failed validation"}

        now_ms = int(time.time() * 1000)
        with self.lock:
            if batch_id in self.seen_batches:
                self.deduped_batches += 1
                return 200, {
                    "ok": True,
                    "batch_id": batch_id,
                    "deduped": True,
                    "accepted": {"nodes": 0, "errors": 0, "transactions": 0, "scores": 0},
                }

            for n in nodes:
                node_id = n.get("node_id")
                if not node_id:
                    continue
                jitter = n.get("input_jitter") or {}
                netplay = n.get("netplay") or {}
                credits = n.get("credits") or {}
                self.conn.execute(
                    """INSERT INTO machine_analytics (
                        venue_id, node_id, hardware, soc, cpu_temp_c, gpu_temp_c, npu_load_pct,
                        active_core, active_rom, frame_time_ms, fps,
                        jitter_poll_hz, jitter_p50_us, jitter_p95_us, jitter_p99_us, jitter_max_us, jitter_dropped,
                        netplay_peer_id, netplay_rtt_ms, netplay_jitter_ms, netplay_loss_pct, netplay_rollback,
                        credits_available, credits_total, credits_today, captured_at_ms, ingested_at_ms
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                    (
                        venue,
                        node_id,
                        n.get("hardware"),
                        n.get("soc"),
                        n.get("cpu_temp_c"),
                        n.get("gpu_temp_c"),
                        n.get("npu_load_pct"),
                        n.get("active_core"),
                        n.get("active_rom"),
                        n.get("frame_time_ms"),
                        n.get("fps"),
                        jitter.get("poll_rate_hz"),
                        jitter.get("p50_us"),
                        jitter.get("p95_us"),
                        jitter.get("p99_us"),
                        jitter.get("max_us"),
                        jitter.get("dropped_events"),
                        netplay.get("peer_id"),
                        netplay.get("rtt_ms"),
                        netplay.get("jitter_ms"),
                        netplay.get("packet_loss_pct"),
                        netplay.get("rollback_frames"),
                        credits.get("credits_available"),
                        credits.get("credits_inserted_total"),
                        credits.get("credits_dispensed_today"),
                        int(n.get("timestamp_epoch_ms", now_ms)),
                        now_ms,
                    ),
                )
                self.latest_node[node_id] = n

            self.conn.commit()
            self.seen_batches.add(batch_id)
            self.latest_venue[venue] = {
                "venue": venue,
                "generated_at_epoch_ms": generated_at,
                "nodes": nodes,
            }
            self.accepted_batches += 1
            if len(self.sample_envelopes) < 4:
                self.sample_envelopes.append(batch)

        return 200, {
            "ok": True,
            "batch_id": batch_id,
            "deduped": False,
            "accepted": {
                "nodes": len(nodes),
                "errors": len(errors),
                "transactions": len(transactions),
                "scores": len(scores),
            },
        }

    def query_counts(self) -> dict[str, dict[str, Any]]:
        with self.lock:
            rows = self.conn.execute(
                """SELECT node_id, hardware, COUNT(*),
                          SUM(CASE WHEN cpu_temp_c IS NULL THEN 1 ELSE 0 END)
                   FROM machine_analytics
                   GROUP BY node_id, hardware"""
            ).fetchall()
            return {
                row[0]: {
                    "hardware": row[1],
                    "rows": int(row[2]),
                    "null_cpu_temp_rows": int(row[3] or 0),
                }
                for row in rows
            }


class ThreadingEdgeHttpServer(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


def start_edge_mock_server(store: EdgeMockStore) -> tuple[ThreadingEdgeHttpServer, int]:
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, format: str, *args: Any) -> None:
            return

        def _send_json(self, code: int, payload: Any) -> None:
            body = json.dumps(payload).encode("utf-8")
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self) -> None:  # noqa: N802
            if self.path != "/api/v1/edge/ingest":
                self._send_json(404, {"error": "not_found"})
                return
            length = int(self.headers.get("Content-Length", "0"))
            raw = self.rfile.read(length)
            try:
                body = json.loads(raw.decode("utf-8"))
            except Exception:
                self._send_json(400, {"error": "malformed_json"})
                return
            auth = self.headers.get("Authorization", "")
            token = auth[7:].strip() if auth.startswith("Bearer ") else self.headers.get("X-BOSS-Token")
            code, resp = store.ingest(token, body)
            self._send_json(code, resp)

        def do_GET(self) -> None:  # noqa: N802
            parsed = urllib.parse.urlparse(self.path)
            if parsed.path != "/api/v1/edge/latest":
                self._send_json(404, {"error": "not_found"})
                return
            qs = urllib.parse.parse_qs(parsed.query)
            with store.lock:
                if "node" in qs:
                    node = store.latest_node.get(qs["node"][0])
                    if not node:
                        self._send_json(404, {"error": "node_not_found"})
                        return
                    self._send_json(200, node)
                    return
                venue = qs.get("venue", ["Beercade Redfern (The BK)"])[0]
                snap = store.latest_venue.get(venue)
                if not snap:
                    self._send_json(404, {"error": "venue_not_found"})
                    return
                self._send_json(200, snap)

    port = free_tcp_port()
    srv = ThreadingEdgeHttpServer(("127.0.0.1", port), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, port


def make_telemetry(node_id: str, hardware: str, soc: str, cpu_temp: Optional[float], gpu_temp: Optional[float]) -> dict[str, Any]:
    return {
        "node_id": node_id,
        "hardware": hardware,
        "soc": soc,
        "cpu_temp_c": cpu_temp,
        "gpu_temp_c": gpu_temp,
        "npu_load_pct": 12.5,
        "active_core": "snes9x",
        "active_rom": "dkc.zip",
        "frame_time_ms": 16.6,
        "fps": 60.0,
        "input_jitter": {
            "poll_rate_hz": 1000.0,
            "p50_us": 42.0,
            "p95_us": 110.0,
            "p99_us": 185.0,
            "max_us": 310.0,
            "dropped_events": 0,
        },
        "netplay": None,
        "credits": {
            "credits_available": 4,
            "credits_inserted_total": 12,
            "credits_dispensed_today": 8,
            "last_coin_epoch_ms": None,
        },
        "mode": "Standalone",
        "timestamp_epoch_ms": int(time.time() * 1000),
    }


def run_brc_session_lifecycle(base_url: str, node_id: str, node_token: str, rpc_token: str) -> dict[str, Any]:
    # 1. Start session via activate_game_time_pass
    status, _, body = http_request(
        f"{base_url}/api/v1/plugins/pos-payments/action",
        method="POST",
        json_body={
            "action": "activate_game_time_pass",
            "payload": {
                "node_id": node_id,
                "duration_mins": 30,
                "credits": 10,
                "topup_grace_secs": 90,
                "game_ids": ["dkc", "sfiii3"],
            },
        },
    )
    if status != 200:
        raise RuntimeError(f"activate_game_time_pass failed ({status}): {body.decode('utf-8', 'replace')}")
    res = json.loads(body.decode("utf-8"))["result"]
    code = res["session_code"]
    session_id = res["session_id"]
    if not code.startswith("BRC-"):
        raise RuntimeError(f"Unexpected session_code: {code}")

    # 2. Public patron lookup (case-insensitive)
    status, _, body = http_request(f"{base_url}/api/v1/sessions/{code.lower()}")
    if status != 200:
        raise RuntimeError(f"session lookup failed ({status}): {body.decode('utf-8', 'replace')}")
    view = json.loads(body.decode("utf-8"))
    if not view.get("ok") or view.get("in_grace") is not False:
        raise RuntimeError(f"Unexpected initial session view: {view}")

    # 3. Cabinet reports SessionExpired -> enters grace window
    status, _, body = http_request(
        f"{base_url}/api/v1/cabinet/{node_id}/events",
        method="POST",
        headers={"Authorization": f"Bearer {node_token}"},
        json_body={"type": "session_expired", "session_id": session_id},
    )
    if status != 200:
        raise RuntimeError(f"session_expired event failed ({status}): {body.decode('utf-8', 'replace')}")

    status, _, body = http_request(f"{base_url}/api/v1/sessions/{code}")
    grace_view = json.loads(body.decode("utf-8"))
    if status != 200 or grace_view.get("in_grace") is not True:
        raise RuntimeError(f"Session did not enter grace window: {grace_view}")

    # 4. Top-up & swap games via POST /api/v1/sessions/:code/update
    status, _, body = http_request(
        f"{base_url}/api/v1/sessions/{code}/update",
        method="POST",
        json_body={"game_ids": ["batman_returns", "mvc2"], "add_secs": 900},
    )
    if status != 200:
        raise RuntimeError(f"session update failed ({status}): {body.decode('utf-8', 'replace')}")
    upd = json.loads(body.decode("utf-8"))
    if not upd.get("ok") or upd.get("session", {}).get("in_grace") is not False:
        raise RuntimeError(f"Unexpected post-update session view: {upd}")

    # 5. Poll queued commands on cabinet endpoint
    status, _, body = http_request(
        f"{base_url}/api/v1/cabinet/{node_id}/commands",
        headers={"Authorization": f"Bearer {node_token}"},
    )
    if status != 200:
        raise RuntimeError(f"poll commands failed ({status}): {body.decode('utf-8', 'replace')}")
    cmds = json.loads(body.decode("utf-8")).get("commands", [])
    cmd_types = [c.get("command", {}).get("type") for c in cmds]
    if "start_session" not in cmd_types or "update_session" not in cmd_types:
        raise RuntimeError(f"Missing expected session commands in queue for {node_id}: {cmd_types}")

    # 6. Operator ends session & verifies 404 on subsequent lookup
    status, _, body = http_request(
        f"{base_url}/api/v1/sessions/{code}/end",
        method="POST",
        headers={"Authorization": f"Bearer {rpc_token}"},
    )
    if status != 200:
        raise RuntimeError(f"end session failed ({status}): {body.decode('utf-8', 'replace')}")
    status, _, _ = http_request(f"{base_url}/api/v1/sessions/{code}")
    if status != 404:
        raise RuntimeError(f"Ended session {code} still returned HTTP {status} (expected 404)")

    return {"session_code": code, "session_id": session_id, "commands": cmd_types}


def build_netplay_header(msg_type: int, session_id: int) -> bytes:
    return MAGIC + bytes([WIRE_VER, msg_type]) + struct.pack("<Q", session_id)


def run_netplay_relay_probe(base_url: str, host: str, port: int, node_a: str, node_b: str) -> dict[str, Any]:
    session_id = 0x4D355F534F414B01
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock_a, socket.socket(
        socket.AF_INET, socket.SOCK_DGRAM
    ) as sock_b:
        sock_a.settimeout(3.0)
        sock_b.settimeout(3.0)
        addr = (host, port)

        def announce(sock: socket.socket, node_id: str) -> bytes:
            nb = node_id.encode("utf-8")
            pkt = build_netplay_header(MSG_ANNOUNCE, session_id) + bytes([len(nb)]) + nb
            sock.sendto(pkt, addr)
            resp, _ = sock.recvfrom(1500)
            if len(resp) < 14 or resp[0:2] != MAGIC or resp[3] != MSG_ANNOUNCE_ACK:
                raise RuntimeError(f"Bad ANNOUNCE_ACK for {node_id}: {resp.hex()}")
            return resp

        ack_a = announce(sock_a, node_a)
        ack_b = announce(sock_b, node_b)
        if ack_b[13] != 4:
            raise RuntimeError(f"Second station ACK missing peer IPv4 (flag={ack_b[13]}): {ack_b.hex()}")

        # Watchdog + synthetic rollback input relay A -> B and B -> A
        wd_a = build_netplay_header(MSG_WATCHDOG, session_id) + struct.pack("<BHHH", 0, 0, 5250, 5850)
        wd_b = build_netplay_header(MSG_WATCHDOG, session_id) + struct.pack("<BHHH", 0, 0, 5420, 5100)
        in_a = build_netplay_header(MSG_INPUT, session_id) + b"FRAME1-X86"
        in_b = build_netplay_header(MSG_INPUT, session_id) + b"FRAME1-ARM"

        sock_a.sendto(wd_a, addr)
        sock_a.sendto(in_a, addr)
        got_b1, _ = sock_b.recvfrom(1500)
        got_b2, _ = sock_b.recvfrom(1500)

        sock_b.sendto(wd_b, addr)
        sock_b.sendto(in_b, addr)
        got_a1, _ = sock_a.recvfrom(1500)
        got_a2, _ = sock_a.recvfrom(1500)

        if in_a not in (got_b1, got_b2) or in_b not in (got_a1, got_a2):
            raise RuntimeError("Relayed UDP netplay input packets did not arrive intact at peer")

    status, _, body = http_request(f"{base_url}/api/v1/netplay/sessions")
    if status != 200:
        raise RuntimeError(f"GET /api/v1/netplay/sessions failed ({status})")
    sessions = json.loads(body.decode("utf-8"))
    matched = [s for s in sessions if s.get("session_id") == session_id]
    if not matched:
        raise RuntimeError(f"Netplay session {session_id} missing from /api/v1/netplay/sessions")
    summary = matched[0]
    if len(summary.get("endpoints", [])) != 2 or summary.get("relayed_packets", 0) < 4:
        raise RuntimeError(f"Unexpected netplay relay summary: {summary}")
    if summary.get("desync_events", 0) != 0:
        raise RuntimeError(f"Unexpected netplay desyncs: {summary}")
    return {
        "session_id": session_id,
        "ack_a_slot": ack_a[12],
        "ack_b_slot": ack_b[12],
        "relayed_packets": summary["relayed_packets"],
        "endpoints": [e["node_id"] for e in summary["endpoints"]],
    }


def run_ota_upload_download_probe(base_url: str, rpc_token: str, payload_bytes: Optional[bytes] = None, filename: str = "m5-soak-test.sqsh") -> dict[str, Any]:
    if payload_bytes is None:
        # 256 KiB deterministic SquashFS-magic payload
        block = bytes((i * 31 + 7) & 0xFF for i in range(4096))
        payload_bytes = b"hsqs" + (block * 64)[4:]
    expected_sha = hashlib.sha256(payload_bytes).hexdigest()

    status, _, body = http_request(
        f"{base_url}/api/v1/sync/upload",
        method="POST",
        headers={
            "Authorization": f"Bearer {rpc_token}",
            "Content-Type": "application/octet-stream",
            "Content-Length": str(len(payload_bytes)),
            "X-Payload-Name": filename,
        },
        raw_body=payload_bytes,
    )
    if status != 200:
        raise RuntimeError(f"OTA upload failed ({status}): {body.decode('utf-8', 'replace')}")

    status, _, body = http_request(
        f"{base_url}/api/v1/sync",
        headers={"Authorization": f"Bearer {rpc_token}"},
    )
    if status != 200:
        raise RuntimeError(f"GET /api/v1/sync failed ({status})")
    listing = json.loads(body.decode("utf-8")).get("payloads", [])
    found = [p for p in listing if p.get("filename") == filename]
    if not found or found[0].get("sha256") != expected_sha:
        raise RuntimeError(f"OTA listing mismatch for {filename}: {found}")

    status, hdrs, dl_bytes = http_request(f"{base_url}/api/v1/sync/{filename}")
    if status != 200:
        raise RuntimeError(f"GET /api/v1/sync/{filename} failed ({status})")
    hdr_sha = hdrs.get("x-ota-sha256", "")
    actual_sha = hashlib.sha256(dl_bytes).hexdigest()
    if hdr_sha != expected_sha or actual_sha != expected_sha:
        raise RuntimeError(
            f"OTA SHA-256 mismatch: expected={expected_sha}, header={hdr_sha}, body={actual_sha}"
        )
    return {"filename": filename, "size_bytes": len(dl_bytes), "sha256": actual_sha}


def write_report(report_path: Path, report: dict[str, Any]) -> None:
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")


def run_smoke_mode(args: argparse.Namespace) -> int:
    boss_bin = find_boss_binary()
    node_token = args.node_token
    rpc_token = args.rpc_token
    edge_token = args.edge_token
    boss_port = free_tcp_port()
    relay_port = free_udp_port()
    base_url = f"http://127.0.0.1:{boss_port}"

    tmp_dir = Path(tempfile.mkdtemp(prefix="beercade-m5-smoke-"))
    sync_dir = tmp_dir / "sync"
    plugins_dir = tmp_dir / "plugins"
    static_dir = tmp_dir / "kiosk_ui"
    sync_dir.mkdir()
    plugins_dir.mkdir()
    static_dir.mkdir()
    (static_dir / "index.html").write_text("<html><body>beercade-m5-smoke</body></html>", encoding="utf-8")

    store = EdgeMockStore(tmp_dir / "edge_d1.sqlite3", edge_token)
    edge_srv, edge_port = start_edge_mock_server(store)

    env = os.environ.copy()
    env.update(
        {
            "BOSS_HOST": "127.0.0.1",
            "BOSS_PORT": str(boss_port),
            "BOSS_NETPLAY_UDP_PORT": str(relay_port),
            "BOSS_NODE_TOKEN": node_token,
            "BOSS_RPC_TOKEN": rpc_token,
            "BOSS_SYNC_DIR": str(sync_dir),
            "BOSS_PLUGINS_DIR": str(plugins_dir),
            "BOSS_STATIC_DIR": str(static_dir),
            "EDGE_INGEST_URL": f"http://127.0.0.1:{edge_port}/api/v1/edge/ingest",
            "EDGE_INGEST_TOKEN": edge_token,
            "RUST_LOG": env.get("RUST_LOG", "warn"),
        }
    )

    boss_log = (tmp_dir / "boss.log").open("w", encoding="utf-8")
    proc = subprocess.Popen(
        [str(boss_bin)],
        cwd=str(REPO_ROOT),
        env=env,
        stdout=boss_log,
        stderr=subprocess.STDOUT,
    )

    report: dict[str, Any] = {
        "mode": "smoke",
        "started_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "boss_binary": str(boss_bin),
        "boss_port": boss_port,
        "netplay_udp_port": relay_port,
        "edge_port": edge_port,
        "status": "failed",
        "planes": {},
    }

    try:
        # Wait for B.O.S.S. health
        deadline = time.monotonic() + 15.0
        healthy = False
        while time.monotonic() < deadline:
            if proc.poll() is not None:
                raise RuntimeError(f"boss-server exited early with code {proc.returncode}")
            try:
                st, _, body = http_request(f"{base_url}/api/health", timeout=2.0)
                if st == 200 and body.decode("utf-8", "replace").strip() == "ok":
                    healthy = True
                    break
            except Exception:
                pass
            time.sleep(0.25)
        if not healthy:
            raise RuntimeError("Timed out waiting for boss-server /api/health")

        node_x86 = "CAB-X86-SOAK"
        node_arm = "CAB-ARM-SOAK"
        auth_node = {"Authorization": f"Bearer {node_token}"}

        # Send mixed-arch telemetry over duration_secs
        end_at = time.monotonic() + max(args.duration_secs, 6)
        heartbeats_sent = 0
        sessions_completed: list[dict[str, Any]] = []
        dropped_sessions = 0

        # Run initial BRC-XXXX session cycles across both architectures
        for nid in (node_x86, node_arm):
            try:
                sessions_completed.append(run_brc_session_lifecycle(base_url, nid, node_token, rpc_token))
            except Exception:
                dropped_sessions += 1
                raise

        # Run UDP :9750 netplay relay verification
        netplay_res = run_netplay_relay_probe(base_url, "127.0.0.1", relay_port, node_x86, node_arm)

        # Run OTA upload + X-OTA-SHA256 verification
        ota_res = run_ota_upload_download_probe(base_url, rpc_token)

        while time.monotonic() < end_at:
            for nid, hw, soc, cpu_t, gpu_t in (
                (node_x86, "x86_64", "x86_64", None, 58.5),
                (node_arm, "aarch64", "rk3588", 54.2, 51.0),
            ):
                st, _, _ = http_request(
                    f"{base_url}/api/v1/telemetry",
                    method="POST",
                    headers=auth_node,
                    json_body=make_telemetry(nid, hw, soc, cpu_t, gpu_t),
                )
                if st != 200:
                    raise RuntimeError(f"POST /api/v1/telemetry for {nid} returned {st}")
                heartbeats_sent += 1
            time.sleep(2.0)

        # Verify GET /api/telemetry reports both online
        st, _, snap_raw = http_request(f"{base_url}/api/telemetry")
        if st != 200:
            raise RuntimeError(f"GET /api/telemetry failed ({st})")
        snap = json.loads(snap_raw.decode("utf-8"))
        online_ids = {n["node_id"] for n in snap.get("nodes", [])}
        if not {node_x86, node_arm}.issubset(online_ids):
            raise RuntimeError(f"Missing expected nodes in /api/telemetry: {online_ids}")

        # Wait up to 10s for B.O.S.S. 5s edge forwarder flush into SQLite D1 mock
        flush_deadline = time.monotonic() + 10.0
        counts: dict[str, dict[str, Any]] = {}
        while time.monotonic() < flush_deadline:
            counts = store.query_counts()
            if node_x86 in counts and node_arm in counts:
                break
            time.sleep(0.5)

        if node_x86 not in counts or node_arm not in counts:
            raise RuntimeError(f"D1 edge ingest missing mixed-arch nodes: {counts}")
        if counts[node_x86]["hardware"] != "x86_64" or counts[node_x86]["null_cpu_temp_rows"] < 1:
            raise RuntimeError(f"x86_64 D1 parity/nullable thermal mismatch: {counts[node_x86]}")
        if counts[node_arm]["hardware"] != "aarch64" or counts[node_arm]["null_cpu_temp_rows"] != 0:
            raise RuntimeError(f"aarch64 D1 parity mismatch: {counts[node_arm]}")
        if store.rejected_batches != 0 or store.accepted_batches < 1:
            raise RuntimeError(
                f"Unexpected edge batch stats: accepted={store.accepted_batches}, rejected={store.rejected_batches}"
            )

        # Verify KV latest endpoint parity & batch_id dedupe idempotency
        st_kv, _, kv_raw = http_request(f"{base_url.replace(str(boss_port), str(edge_port))}/api/v1/edge/latest?node={node_x86}")
        if st_kv != 200 or json.loads(kv_raw.decode("utf-8")).get("hardware") != "x86_64":
            raise RuntimeError(f"KV latest parity failed ({st_kv})")

        sample_env = store.sample_envelopes[0]
        st_dup, _, dup_raw = http_request(
            f"http://127.0.0.1:{edge_port}/api/v1/edge/ingest",
            method="POST",
            headers={"Authorization": f"Bearer {edge_token}"},
            json_body=sample_env,
        )
        if st_dup != 200 or json.loads(dup_raw.decode("utf-8")).get("deduped") is not True:
            raise RuntimeError(f"Edge batch deduplication check failed ({st_dup}): {dup_raw!r}")

        report["planes"] = {
            "telemetry_and_edge_d1_kv": {
                "heartbeats_sent": heartbeats_sent,
                "accepted_batches": store.accepted_batches,
                "deduped_batches": store.deduped_batches,
                "rejected_batches": store.rejected_batches,
                "d1_counts": counts,
            },
            "patron_sessions": {
                "completed_sessions": len(sessions_completed),
                "dropped_sessions": dropped_sessions,
                "details": sessions_completed,
            },
            "netplay_udp_relay": netplay_res,
            "ota_sync": ota_res,
        }
        report["status"] = "passed"
        write_report(Path(args.report_path), report)
        print("[PASS] M5 mixed-arch venue smoke soak passed all 4 planes.")
        print(json.dumps(report["planes"], indent=2))
        return 0

    except Exception as exc:
        report["error"] = str(exc)
        write_report(Path(args.report_path), report)
        print(f"[FAIL] M5 smoke soak failed: {exc}", file=sys.stderr)
        return 1

    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=5)
        boss_log.close()
        edge_srv.shutdown()
        edge_srv.server_close()
        store.close()
        shutil.rmtree(tmp_dir, ignore_errors=True)


def run_venue_mode(args: argparse.Namespace) -> int:
    base_url = args.boss_url.rstrip("/")
    parsed = urllib.parse.urlparse(base_url)
    netplay_host = args.netplay_host or (parsed.hostname or "127.0.0.1")
    expected_nodes = [n.strip() for n in args.nodes.split(",") if n.strip()] if args.nodes else []

    start_ts = time.monotonic()
    deadline = start_ts + args.duration_secs
    last_checkpoint = start_ts
    polls = 0
    health_failures = 0
    mixed_arch_seen = {"x86_64": False, "aarch64": False}
    dropped_sessions = 0
    sessions_completed = 0
    max_desyncs = 0
    max_relayed = 0
    ota_uploaded: Optional[dict[str, Any]] = None

    if args.ota_image:
        ota_path = Path(args.ota_image)
        if not ota_path.is_file():
            print(f"[FAIL] --ota-image not found: {ota_path}", file=sys.stderr)
            return 3
        ota_uploaded = run_ota_upload_download_probe(
            base_url,
            args.rpc_token,
            payload_bytes=ota_path.read_bytes(),
            filename=ota_path.name,
        )
        print(f"[INFO] Uploaded & verified OTA payload: {ota_uploaded}")

    report: dict[str, Any] = {
        "mode": "venue",
        "boss_url": base_url,
        "netplay_target": f"{netplay_host}:{args.netplay_port}",
        "expected_nodes": expected_nodes,
        "duration_secs": args.duration_secs,
        "status": "running",
    }

    while True:
        polls += 1
        try:
            st_h, _, b_h = http_request(f"{base_url}/api/health", timeout=5.0)
            if st_h != 200 or b_h.decode("utf-8", "replace").strip() != "ok":
                health_failures += 1

            st_t, _, b_t = http_request(f"{base_url}/api/telemetry", timeout=5.0)
            if st_t == 200:
                snap = json.loads(b_t.decode("utf-8"))
                nodes = snap.get("nodes", [])
                for n in nodes:
                    hw = (n.get("hardware") or "").lower()
                    if "x86" in hw:
                        mixed_arch_seen["x86_64"] = True
                    if "aarch64" in hw or "arm" in hw:
                        mixed_arch_seen["aarch64"] = True

                if not args.observe_only and (polls == 1 or polls % 60 == 0) and nodes:
                    target_node = expected_nodes[0] if expected_nodes else nodes[0]["node_id"]
                    try:
                        run_brc_session_lifecycle(base_url, target_node, args.node_token, args.rpc_token)
                        sessions_completed += 1
                    except Exception as exc:
                        dropped_sessions += 1
                        print(f"[WARN] BRC session probe failed on {target_node}: {exc}", file=sys.stderr)

            st_np, _, b_np = http_request(f"{base_url}/api/v1/netplay/sessions", timeout=5.0)
            if st_np == 200:
                for sess in json.loads(b_np.decode("utf-8")):
                    max_desyncs = max(max_desyncs, int(sess.get("desync_events", 0)))
                    max_relayed = max(max_relayed, int(sess.get("relayed_packets", 0)))

            if args.edge_latest_url:
                st_e, _, _ = http_request(args.edge_latest_url, timeout=8.0)
                if st_e != 200:
                    health_failures += 1
        except Exception as exc:
            health_failures += 1
            print(f"[WARN] Poll {polls} exception: {exc}", file=sys.stderr)

        now = time.monotonic()
        if now - last_checkpoint >= 300.0 or now >= deadline:
            report.update(
                {
                    "elapsed_secs": round(now - start_ts, 1),
                    "polls": polls,
                    "health_failures": health_failures,
                    "mixed_arch_seen": mixed_arch_seen,
                    "sessions_completed": sessions_completed,
                    "dropped_sessions": dropped_sessions,
                    "netplay_max_relayed": max_relayed,
                    "netplay_desync_events": max_desyncs,
                    "ota_uploaded": ota_uploaded,
                }
            )
            write_report(Path(args.report_path), report)
            last_checkpoint = now

        if now >= deadline:
            break
        time.sleep(min(30.0, max(1.0, deadline - now)))

    passed = (
        health_failures == 0
        and dropped_sessions == 0
        and mixed_arch_seen["x86_64"]
        and mixed_arch_seen["aarch64"]
    )
    report["status"] = "passed" if passed else "failed"
    write_report(Path(args.report_path), report)
    if passed:
        print(f"[PASS] M5 venue soak passed ({polls} polls, {sessions_completed} sessions, 0 dropped).")
        return 0
    print(f"[FAIL] M5 venue soak failed criteria: {report}", file=sys.stderr)
    return 1


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="M5 Mixed-Arch Venue Soak & Acceptance Harness")
    parser.add_argument("--mode", choices=["smoke", "venue"], default="smoke")
    parser.add_argument("--duration-secs", type=int, default=None)
    parser.add_argument("--boss-url", default="http://127.0.0.1:8080")
    parser.add_argument("--netplay-host", default="")
    parser.add_argument("--netplay-port", type=int, default=9750)
    parser.add_argument("--node-token", default=os.environ.get("BOSS_NODE_TOKEN", "m5-soak-node-token"))
    parser.add_argument("--rpc-token", default=os.environ.get("BOSS_RPC_TOKEN", "m5-soak-rpc-token"))
    parser.add_argument("--edge-token", default=os.environ.get("EDGE_INGEST_TOKEN", "m5-soak-edge-token"))
    parser.add_argument("--edge-latest-url", default="")
    parser.add_argument("--nodes", default="")
    parser.add_argument("--ota-image", default="")
    parser.add_argument("--observe-only", action="store_true")
    parser.add_argument("--report-path", default=str(REPO_ROOT / "out" / "m5-soak-report.json"))
    args = parser.parse_args()
    if args.duration_secs is None:
        args.duration_secs = 15 if args.mode == "smoke" else 86400
    return args


def main() -> int:
    args = parse_args()
    if args.mode == "smoke":
        return run_smoke_mode(args)
    return run_venue_mode(args)


if __name__ == "__main__":
    raise SystemExit(main())
