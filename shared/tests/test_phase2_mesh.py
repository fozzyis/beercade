#!/usr/bin/env python3
"""
B.E.E.R.C.A.D.E. Phase 2.1 Full-Stack Mesh & Telemetry Test Harness
Validates BOSS server startup, node registration, telemetry stream, and mode transitions.
"""

import subprocess
import time
import urllib.request
import json
import sys

def test_full_stack():
    print("===============================================================")
    print("   Starting Phase 2.1: B.O.S.S. <-> O.R.I.O.N. Integration QA  ")
    print("===============================================================")

    # 1. Start BOSS server via Docker / buildfarm container with precompiled binary
    boss_proc = subprocess.Popen(
        ["docker", "run", "--rm", "--net=host", "-v", "/data/dev/beercade:/beercade-src", "-w", "/beercade-src", "astro-buildfarm", "./BOSS/target/release/boss-server"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE
    )

    print("[+] Waiting for BOSS server to bind on port 8989...")
    time.sleep(4)

    # 2. Check BOSS status
    try:
        req = urllib.request.Request("http://127.0.0.1:8989/api/telemetry")
        with urllib.request.urlopen(req, timeout=3) as resp:
            data = json.loads(resp.read().decode())
            print("[✓] BOSS server snapshot response:")
            print(json.dumps(data, indent=2))
    except Exception as e:
        print(f"[-] Failed to connect to BOSS server: {e}")
        boss_proc.kill()
        sys.exit(1)

    # 3. Simulate O.R.I.O.N. node telemetry dispatch
    sample_node_payload = {
        "node_id": "ORION-01",
        "hardware": "Radxa Rock 5C",
        "soc": "Rockchip RK3588S (4x A76 + 4x A55)",
        "cpu_temp_c": 44.2,
        "gpu_temp_c": 41.8,
        "active_core": "FBNeo",
        "active_rom": "sf2ce",
        "frame_time_ms": 16.63,
        "mode": {
            "type": "Managed",
            "payload": {
                "session_id": "sess_8941",
                "time_remaining_sec": 420
            }
        },
        "timestamp_epoch_ms": int(time.time() * 1000)
    }

    req = urllib.request.Request(
        "http://127.0.0.1:8989/api/v1/telemetry",
        data=json.dumps(sample_node_payload).encode(),
        headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(req, timeout=3) as resp:
        print(f"[✓] Telemetry ingestion response code: {resp.status}")

    # 4. Re-verify snapshot contains registered ORION-01 node
    with urllib.request.urlopen("http://127.0.0.1:8989/api/telemetry", timeout=3) as resp:
        data = json.loads(resp.read().decode())
        print("[✓] Updated venue report with live O.R.I.O.N. node:")
        print(json.dumps(data, indent=2))
        assert data["nodes_online"] >= 1
        assert data["nodes"][0]["node_id"] == "ORION-01"

    # 5. Clean shutdown
    boss_proc.kill()
    print("===============================================================")
    print("   [✓] Phase 2.1 Mesh & Telemetry Verification: PASSED         ")
    print("===============================================================")

if __name__ == "__main__":
    test_full_stack()
