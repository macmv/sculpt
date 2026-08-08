"""Dependency-free encoder for Sculpt Live full mesh snapshots."""

from __future__ import annotations

from array import array
from dataclasses import dataclass
import math
import struct
import sys

from mathutils import Vector


MAGIC = b"SCLP"
FULL_SNAPSHOT = 1
# SCLP's header is fixed at 130 bytes; see ../protocol.md.
HEADER = struct.Struct("<4sHQIII16f6ff3i")


@dataclass(frozen=True)
class MeshSnapshot:
    payload: bytes
    vertex_count: int
    triangle_count: int


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


def build_mesh_snapshot(_source, evaluated, mesh, revision: int, settings) -> MeshSnapshot:
    """Serialize an evaluated mesh as a complete SCLP snapshot.

    Vertices are local to the evaluated object; `matrix_world` in the header
    maps them to world space. The coordinate settings then map Blender world
    space to Minecraft block space. Dyntopo topology changes are naturally
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
        raise ValueError("mesh is too large for the Sculpt Live protocol")

    units_per_block = settings.blender_units_per_block
    if not math.isfinite(units_per_block) or units_per_block <= 0:
        raise ValueError("Blender Units per Block must be positive")

    positions = array("f")
    for vertex in mesh.vertices:
        positions.extend(vertex.co)

    indices = array("I")
    material_ids = array("I")
    for triangle in mesh.loop_triangles:
        indices.extend(triangle.vertices)
        material_ids.append(mesh.polygons[triangle.polygon_index].material_index)

    transform = tuple(value for row in evaluated.matrix_world for value in row)
    dirty_aabb = _world_bounds(evaluated)
    numeric_values = (*transform, *dirty_aabb, units_per_block)
    if not all(math.isfinite(value) for value in numeric_values):
        raise ValueError("mesh transform and bounds must contain only finite values")
    header = HEADER.pack(
        MAGIC,
        HEADER.size,
        revision,
        FULL_SNAPSHOT,
        vertex_count,
        triangle_count,
        *transform,
        *dirty_aabb,
        units_per_block,
        *settings.minecraft_origin,
    )
    payload = header + _as_little_endian(positions) + _as_little_endian(indices) + _as_little_endian(material_ids)
    return MeshSnapshot(payload, vertex_count, triangle_count)
