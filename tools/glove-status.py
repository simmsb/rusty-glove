#!/usr/bin/env python3
"""Show how a half or the dongle is doing, from its status page.

The dongle serves its own on its USB side, and forwards a port to each half's.
See `shared::net::port::STATUS`.
"""

import argparse
import re
import shutil
import socket
import subprocess
import sys
from pathlib import Path

PORTS = {"dongle": 7400, "left": 7401, "right": 7402}


# a page comes all at once, so stop once it goes quiet: older dongle firmware
# doesn't pass on that the half hung up
IDLE = 2


def fetch(host, port):
    with socket.create_connection((host, port), timeout=10) as s:
        s.settimeout(15)
        page = s.recv(1024)
        s.settimeout(IDLE)
        try:
            while chunk := s.recv(1024):
                page += chunk
        except TimeoutError:
            pass
    if not page:
        raise ConnectionError("it hung up without sending anything")
    return page.decode(errors="replace")


def symbols(elf):
    """(start, end, name) of each static in `elf`"""
    nm = shutil.which("rust-nm") or shutil.which("arm-none-eabi-nm")
    if not nm or not elf.exists():
        return []
    out = subprocess.run(
        [nm, "-C", "-S", "--defined-only", str(elf)], capture_output=True, text=True
    ).stdout
    syms = []
    for line in out.splitlines():
        parts = line.split(maxsplit=3)
        # task pools are statics in RAM
        if len(parts) == 4 and parts[2] in "bBdD":
            start, size = int(parts[0], 16), int(parts[1], 16)
            syms.append((start, start + size, parts[3]))
    return syms


def name_tasks(page, elf):
    """Name the tasks on a page by the task pool their address lies in"""
    syms = symbols(elf)
    if not syms:
        return page

    def name(m):
        addr = int(m.group(1), 16)
        for start, end, sym in syms:
            if start <= addr < end:
                return sym.removesuffix("::POOL")
        return m.group(0)

    return re.sub(r"task (0x[0-9a-f]+)", name, page)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "devices", nargs="*", metavar="{dongle,left,right}", help="which to show, all by default"
    )
    parser.add_argument("--host", default="10.42.0.1", help="the dongle's address")
    args = parser.parse_args()
    for device in args.devices:
        if device not in PORTS:
            parser.error(f"unknown device {device!r}")
    args.devices = args.devices or list(PORTS)

    failed = False
    target = Path(__file__).resolve().parent.parent / "target"
    for device in args.devices:
        try:
            # the ELF `just left_bin` etc. kept, it's right as long as that's
            # what was last flashed
            print(name_tasks(fetch(args.host, PORTS[device]), target / f"{device}.syms.elf"))
        except OSError as e:
            print(f"{device}: couldn't get its status: {e}\n")
            failed = True
    sys.exit(failed)


if __name__ == "__main__":
    main()
