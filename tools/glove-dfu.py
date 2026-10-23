#!/usr/bin/env python3
"""Update a half's firmware over the network, through the dongle.

The dongle hands the host an address over USB ethernet, and forwards a port
for each half to that half's DFU service. See `shared::net::dfu` for the
protocol.

After the half reboots into the new image this confirms it. A half that
can't be reached to confirm rolls back the next time it resets, and refuses
further updates until then.
"""

import argparse
import socket
import struct
import sys
import time
import zlib

MAGIC = b"GDFU"
OP_WRITE = 0
OP_CONFIRM = 1

STATUS = {
    0: "ok",
    1: "bad request",
    2: "image too large",
    3: "crc mismatch",
    4: "flash error",
    5: "the running image needs confirming first, use --confirm-only",
}

PORTS = {"left": 7301, "right": 7302}

# how long a half has to come back after rebooting into the update
CONFIRM_DEADLINE = 90
# how long the half may go without taking any more of the image, or without
# answering once it has it all (it reads the image back to check it)
WRITE_TIMEOUT = 120
# small, so what we've sent tracks what the half has taken
SEND_BUFFER = 8 * 1024
CHUNK = 1024

# --slow: pieces small enough that each TCP segment to the half fits in one
# radio frame, so nothing is fragmented. Firmware that keeps a lost fragment's
# reassembly slot for a minute can't take a normal update.
SLOW_CHUNK = 64
SLOW_INTERVAL = 0.03
# the half erases a flash page when it starts writing each 4KiB, which stops
# everything (its radio too) for ~85ms, so leave a gap for it
ERASE_SIZE = 4096
DEVICE_CHUNK = 1024
ERASE_PAUSE = 0.4


class Progress:
    def __init__(self, total):
        self.total = total
        self.start = time.monotonic()
        self.last = 0.0

    def update(self, done, force=False):
        now = time.monotonic()
        if not force and now - self.last < 0.2:
            return
        self.last = now

        elapsed = max(now - self.start, 1e-6)
        rate = done / elapsed
        eta = (self.total - done) / rate if rate > 0 else 0
        width = 30
        filled = width * done // self.total
        bar = "#" * filled + "-" * (width - filled)
        sys.stderr.write(
            f"\r[{bar}] {100 * done // self.total:3d}% "
            f"{done // 1024}/{self.total // 1024} KiB "
            f"{rate / 1024:5.1f} KiB/s eta {int(eta) // 60}:{int(eta) % 60:02d} "
        )
        sys.stderr.flush()

    def finish(self):
        self.update(self.total, force=True)
        sys.stderr.write("\n")


def paced_send(s, data, header_len, bar):
    sent = 0
    while sent < len(data):
        n = s.send(data[sent : sent + SLOW_CHUNK])
        before, sent = sent, sent + n
        if bar:
            bar.update(sent)

        # the half erases once it has the first chunk of each page
        trigger = lambda off: (off - header_len - DEVICE_CHUNK) // ERASE_SIZE
        if sent > header_len + DEVICE_CHUNK and trigger(sent) != trigger(before):
            time.sleep(ERASE_PAUSE)
        else:
            time.sleep(SLOW_INTERVAL)
    return sent


def request(host, port, op, payload=b"", timeout=10.0, progress=False, slow=False):
    with socket.create_connection((host, port), timeout=10) as s:
        s.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, SEND_BUFFER)
        s.settimeout(timeout)

        data = MAGIC + bytes([op]) + payload
        bar = Progress(len(data)) if progress else None
        sent = 0
        try:
            if slow:
                # magic, op, then the length and CRC of a write
                sent = paced_send(s, data, 5 + 8, bar)
            while sent < len(data):
                sent += s.send(data[sent : sent + CHUNK])
                if bar:
                    bar.update(sent)
        except (BrokenPipeError, ConnectionResetError):
            # the half may have answered and hung up early, see if it did
            pass
        finally:
            if bar:
                if sent == len(data):
                    bar.finish()
                else:
                    sys.stderr.write("\n")

        if progress and sent == len(data):
            print("Sent, waiting for it to check the image")

        try:
            status = s.recv(1)
        except ConnectionResetError:
            status = b""

    if not status:
        raise ConnectionError(
            "the connection dropped before it answered, the link to it is probably too lossy"
        )

    return status[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("side", choices=PORTS)
    parser.add_argument("image", nargs="?", help="the application as a flat binary, e.g. target/left.bin")
    parser.add_argument("--host", default="10.42.0.1", help="the dongle's address")
    parser.add_argument("--port", type=int, help="override the forwarded port")
    parser.add_argument("--no-confirm", action="store_true", help="leave the update unconfirmed")
    parser.add_argument(
        "--slow",
        action="store_true",
        help="send without fragmenting, for halves whose firmware stalls on a lost fragment",
    )
    parser.add_argument(
        "--confirm-only",
        action="store_true",
        help="just confirm the running image, e.g. after updating some other way",
    )
    args = parser.parse_args()

    port = args.port or PORTS[args.side]

    if args.confirm_only:
        status = request(args.host, port, OP_CONFIRM)
        if status != 0:
            sys.exit(f"Confirming failed: {STATUS.get(status, status)}")
        print("Confirmed")
        return

    if args.image is None:
        parser.error("an image is needed unless --confirm-only is given")

    with open(args.image, "rb") as f:
        image = f.read()

    # the image starts with the vector table, whose first word is the stack pointer
    if len(image) < 8 or not 0x2000_0000 <= struct.unpack_from("<I", image)[0] <= 0x2004_0000:
        sys.exit(f"{args.image} doesn't look like a flat binary of the application")

    print(f"Sending {len(image)} bytes to the {args.side} half")
    header = struct.pack("<II", len(image), zlib.crc32(image))
    try:
        status = request(
            args.host,
            port,
            OP_WRITE,
            header + image,
            timeout=WRITE_TIMEOUT,
            progress=True,
            slow=args.slow,
        )
    except OSError as e:
        sys.exit(f"Update failed: {e}")
    if status != 0:
        sys.exit(f"Update failed: {STATUS.get(status, status)}")

    if args.no_confirm:
        print("Written, it'll roll back unless it's confirmed before its next reset")
        return

    print("Written, waiting for it to come back up")
    # don't catch it before it's gone down
    time.sleep(5)

    deadline = time.monotonic() + CONFIRM_DEADLINE
    while True:
        try:
            status = request(args.host, port, OP_CONFIRM)
            break
        except OSError:
            if time.monotonic() > deadline:
                sys.exit("Couldn't reach it to confirm the update, it'll roll back next time it resets")
            time.sleep(2)

    if status != 0:
        sys.exit(f"Confirming failed: {STATUS.get(status, status)}")

    print("Confirmed")


if __name__ == "__main__":
    main()
