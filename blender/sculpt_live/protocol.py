"""Dependency-free encoder for Sculpt Live full mesh snapshots."""

from __future__ import annotations

from array import array
from dataclasses import dataclass
import struct
import sys
import uuid

from mathutils import Vector


MAGIC = b"SCLP"
VERSION = 1
FULL_SNAPSHOT = 1
OBJECT_ID_PROPERTY = "_sculpt_live_object_id"
HEADER = struct.Struct("<4sHH16sQIII16f6f")


@dataclass(frozen=True)
class MeshSnapshot:
    payload: bytes
    object_id: str
    vertex_count: int
    triangle_count: int


def _get_object_id(source) -> uuid.UUID:
    """Get or create an identifier that persists in the .blend file."""
    value = source.get(OBJECT_ID_PROPERTY)
    try:
        return uuid.UUID(str(value))
    except (TypeError, ValueError, AttributeError):
        identifier = uuid.uuid4()
        source[OBJECT_ID_PROPERTY] = str(identifier)
        return identifier


def _as_little_endian(values: array) -> bytes:
    if sys.byteorder == "little":
        return values.tobytes()
    values.byteswap()
    return values.tobytes()


def _world_bounds(evaluated) -> tuple[float, float, float, float, float, float]:
    points = [evaluated.matrix_world @ Vector(corner) for corner in evaluated.bound_box]
    return (
        min(point.x for point in points),
        min(point.y for point in points),
        min(point.z for point in points),
        max(point.x for point in points),
        max(point.y for point in points),
        max(point.z for point in points),
    )


def build_mesh_snapshot(source, evaluated, mesh, revision: int) -> MeshSnapshot:
    """Serialize an evaluated mesh as a complete v1 snapshot.

    Vertices are local to the evaluated object; `matrix_world` in the header
    maps them to world space. Dyntopo topology changes are therefore naturally
    represented by a complete new vertex/index buffer.
    """
    if revision < 1:
        raise ValueError("snapshot revision must be positive")
    if array("I").itemsize != 4:
        raise RuntimeError("this Python does not provide 32-bit unsigned arrays")

    mesh.calc_loop_triangles()
    vertex_count = len(mesh.vertices)
    triangle_count = len(mesh.loop_triangles)
    if vertex_count == 0 or triangle_count == 0:
        raise ValueError("mesh must contain at least one triangle")
    if vertex_count > 0xFFFFFFFF or triangle_count > 0xFFFFFFFF:
        raise ValueError("mesh is too large for protocol v1")

    positions = array("f")
    for vertex in mesh.vertices:
        positions.extend(vertex.co)

    indices = array("I")
    material_ids = array("I")
    for triangle in mesh.loop_triangles:
        indices.extend(triangle.vertices)
        material_ids.append(mesh.polygons[triangle.polygon_index].material_index)

    identifier = _get_object_id(source)
    transform = tuple(value for row in evaluated.matrix_world for value in row)
    dirty_aabb = _world_bounds(evaluated)
    header = HEADER.pack(
        MAGIC,
        VERSION,
        HEADER.size,
        identifier.bytes,
        revision,
        FULL_SNAPSHOT,
        vertex_count,
        triangle_count,
        *transform,
        *dirty_aabb,
    )
    payload = header + _as_little_endian(positions) + _as_little_endian(indices) + _as_little_endian(material_ids)
    return MeshSnapshot(payload, str(identifier), vertex_count, triangle_count)
