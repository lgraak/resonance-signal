#!/usr/bin/env python3
"""Minimal dependency-free Resonance Signal v1 diagnostic consumer."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import socket
import struct
import time
import urllib.parse
import urllib.request


def discover(base_url: str) -> dict:
    with urllib.request.urlopen(f"{base_url}/v1/sources", timeout=5) as response:
        return json.load(response)


def connect_websocket(host: str, port: int, path: str) -> tuple[socket.socket, object]:
    key = base64.b64encode(os.urandom(16)).decode("ascii")
    request = (
        f"GET {path} HTTP/1.1\r\n"
        f"Host: {host}:{port}\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        "Sec-WebSocket-Version: 13\r\n\r\n"
    )
    connection = socket.create_connection((host, port), timeout=5)
    connection.sendall(request.encode("ascii"))
    reader = connection.makefile("rb")
    status = reader.readline(16_385).decode("iso-8859-1").rstrip("\r\n")
    if " 101 " not in status:
        reader.close()
        connection.close()
        raise RuntimeError(f"WebSocket upgrade failed: {status}")
    fields = {}
    header_bytes = len(status)
    while True:
        line_bytes = reader.readline(16_385)
        header_bytes += len(line_bytes)
        if header_bytes > 16_384:
            raise RuntimeError("WebSocket response headers exceeded diagnostic bound")
        if line_bytes in (b"\r\n", b"\n", b""):
            break
        line = line_bytes.decode("iso-8859-1").rstrip("\r\n")
        if ":" in line:
            name, value = line.split(":", 1)
            fields[name.strip().lower()] = value.strip()
    expected = base64.b64encode(
        hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode("ascii")).digest()
    ).decode("ascii")
    if fields.get("sec-websocket-accept") != expected:
        reader.close()
        connection.close()
        raise RuntimeError("WebSocket accept proof did not match")
    return connection, reader


def receive_exact(reader: object, length: int) -> bytes:
    data = bytearray()
    while len(data) < length:
        chunk = reader.read(length - len(data))
        if not chunk:
            raise RuntimeError("connection closed unexpectedly")
        data.extend(chunk)
    return bytes(data)


def receive_frame(reader: object) -> tuple[int, bytes]:
    first, second = receive_exact(reader, 2)
    if first & 0x70:
        raise RuntimeError("reserved WebSocket bits were set")
    opcode = first & 0x0F
    length = second & 0x7F
    if second & 0x80:
        raise RuntimeError("server frames must not be masked")
    if length == 126:
        length = struct.unpack("!H", receive_exact(reader, 2))[0]
    elif length == 127:
        length = struct.unpack("!Q", receive_exact(reader, 8))[0]
    if length > 1_048_576:
        raise RuntimeError("server frame exceeded diagnostic bound")
    return opcode, receive_exact(reader, length)


def send_frame(connection: socket.socket, opcode: int, payload: bytes) -> None:
    mask = os.urandom(4)
    header = bytearray([0x80 | opcode])
    length = len(payload)
    if length < 126:
        header.append(0x80 | length)
    elif length <= 0xFFFF:
        header.append(0x80 | 126)
        header.extend(struct.pack("!H", length))
    else:
        header.append(0x80 | 127)
        header.extend(struct.pack("!Q", length))
    masked = bytes(value ^ mask[index % 4] for index, value in enumerate(payload))
    connection.sendall(bytes(header) + mask + masked)


def parse_waveform(payload: bytes) -> dict:
    if len(payload) < 40 or payload[:4] != b"RSWF":
        raise RuntimeError("invalid waveform magic or truncated header")
    version, header_length = payload[4], payload[5]
    if version != 1 or header_length != 40:
        raise RuntimeError("unsupported waveform frame version")
    sequence, frame_index, time_ns, frame_count, channels = struct.unpack_from(
        "<QQQIH", payload, 8
    )
    expected = 40 + frame_count * channels * 4
    if channels not in (1, 2) or len(payload) != expected:
        raise RuntimeError("waveform frame length or channel count is invalid")
    samples = struct.unpack_from(f"<{frame_count * channels}f", payload, 40)
    return {
        "sequence": sequence,
        "frame_index": frame_index,
        "stream_time_ns": time_ns,
        "frame_count": frame_count,
        "channels": channels,
        "first_samples": samples[: min(4, len(samples))],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", default=48_480, type=int)
    parser.add_argument("--source-id")
    parser.add_argument("--frames", default=3, type=int)
    parser.add_argument(
        "--stall-seconds",
        default=0.0,
        type=float,
        help="open a stream without reading, for bounded-backpressure diagnostics",
    )
    options = parser.parse_args()
    base_url = f"http://{options.host}:{options.port}"
    snapshot = discover(base_url)
    print(json.dumps(snapshot, indent=2))

    if options.source_id:
        selection = "source_id=" + urllib.parse.quote(options.source_id, safe="")
    else:
        selection = "source=default-playback"
    connection, reader = connect_websocket(
        options.host, options.port, f"/v1/waveform?{selection}"
    )
    if options.stall_seconds > 0:
        time.sleep(options.stall_seconds)
        reader.close()
        connection.close()
        return 0
    binary_count = 0
    stop_sent = False
    try:
        while True:
            opcode, payload = receive_frame(reader)
            if opcode == 1:
                event = json.loads(payload)
                print(json.dumps(event, indent=2))
                if event.get("type") == "stream_stopped":
                    if event.get("reason") != "consumer_cancelled":
                        raise RuntimeError("diagnostic stop was not reported cleanly")
                    return 0
                if event.get("type") == "stream_error":
                    return 2
            elif opcode == 2:
                print(parse_waveform(payload))
                binary_count += 1
                if binary_count >= options.frames and not stop_sent:
                    send_frame(connection, 1, b'{"type":"stop"}')
                    stop_sent = True
            elif opcode == 8:
                raise RuntimeError("server closed before stream_stopped")
            elif opcode == 9:
                send_frame(connection, 10, payload)
    finally:
        try:
            send_frame(connection, 8, struct.pack("!H", 1000))
        except OSError:
            pass
        reader.close()
        connection.close()


if __name__ == "__main__":
    raise SystemExit(main())
