"""Local Unix-domain-socket transport for complete snapshot frames."""

import socket
import struct


FRAME_LENGTH = struct.Struct("<I")
MAX_PAYLOAD_SIZE = 4 * 1024 * 1024


def send_snapshot(socket_path: str, payload: bytes) -> None:
    """Send one length-prefixed snapshot frame to the local voxel service."""
    if not socket_path:
        raise ValueError("socket path is required")
    if not payload:
        raise ValueError("snapshot payload must not be empty")
    if len(payload) > MAX_PAYLOAD_SIZE:
        raise ValueError("snapshot exceeds the 4 MiB protocol frame limit")

    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(2.0)
        connection.connect(socket_path)
        connection.sendall(FRAME_LENGTH.pack(len(payload)))
        connection.sendall(payload)
