"""Wait for this EasyInput V2's transient ESP32-S3 USB port, then flash three locked images.

Run only after the user explicitly authorizes the exact hashes below. The script
never erases NVS and makes at most one esptool attempt per invocation.
"""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import subprocess
import sys
import time

from serial.tools import list_ports


ROOT = Path(__file__).resolve().parents[1]
FIRMWARE = ROOT / "firmware"
IMAGES = (
    ("0x0", FIRMWARE / "releases/bootloader-20261005.bin", "be3abea605a6be7f04c2d0f4011bd90688f799a834a164cdc6a29b16c3324287"),
    ("0x8000", FIRMWARE / "releases/partition-table-20261005.bin", "7c541b70dcac8f920c2d11589f06745e1b033fa9b95b8343de2748bb8312a278"),
    ("0x10000", FIRMWARE / "releases/keyboard-volume-20261006.bin", "93d93d9e9177841ba7a91c4a52517587370cf5a272dc2ce274076b203fe85ed7"),
)
def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--identify", action="store_true", help="Only report the transient ESP32-S3 USB serial; never flash")
    mode.add_argument("--serial", help="ESP32-S3 USB serial confirmed for the board being flashed")
    parser.add_argument("--application-only", action="store_true", help="Update only the application at 0x10000; preserve bootloader, partition table and NVS")
    args = parser.parse_args()
    expected_serial = args.serial.upper() if args.serial else None
    images = IMAGES[2:] if args.application_only else IMAGES
    if not args.identify:
        for _, path, expected in images:
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            if digest != expected:
                raise RuntimeError(f"Image SHA-256 changed: {path.name}")
    print("status=waiting_for_esp32s3_usb_port", flush=True)
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        candidates = [
            port for port in list_ports.comports()
            if port.vid == 0x303A and port.pid == 0x1001
        ]
        if len(candidates) > 1:
            raise RuntimeError("Multiple ESP32-S3 USB ports found; refusing to choose")
        if candidates:
            port = candidates[0]
            if not port.serial_number:
                raise RuntimeError("ESP32-S3 USB serial is missing")
            if args.identify:
                print(f"status=identified serial={port.serial_number}", flush=True)
                return 0
            if port.serial_number.upper() != expected_serial:
                raise RuntimeError("ESP32-S3 USB serial does not match the confirmed board identity")
            print(f"status=port_found port={port.device}", flush=True)
            command = [
                sys.executable, "-m", "esptool", "--chip", "esp32s3", "-p", port.device,
                "-b", "460800", "--before", "usb_reset", "--after", "hard_reset",
                "--connect-attempts", "0", "write_flash", "--flash_mode", "dio",
                "--flash_freq", "80m", "--flash_size", "16MB",
            ]
            for offset, path, _ in images:
                command.extend((offset, str(path)))
            return subprocess.run(command, cwd=FIRMWARE, check=False).returncode
        time.sleep(0.05)
    print("status=timed_out_no_port", flush=True)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
