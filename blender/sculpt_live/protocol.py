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
# SCLP's header is fixed at 132 bytes; see ../protocol.md.
HEADER = struct.Struct("<4sHQIII16f6ff3iH")


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


def _nearest_material_id(mesh, polygon, materials) -> int:
    """Choose the configured color closest to a polygon's painted color."""
    attribute = mesh.color_attributes.active_color
    if attribute is None:
        return 0
    if attribute.domain == "POINT":
        samples = (attribute.data[vertex].color for vertex in polygon.vertices)
    else:
        samples = (attribute.data[loop].color for loop in polygon.loop_indices)
    samples = list(samples)
    if not samples:
        return 0
    color = tuple(sum(sample[channel] for sample in samples) / len(samples) for channel in range(3))
    return min(
        range(len(materials)),
        key=lambda index: sum((color[channel] - materials[index].color[channel]) ** 2 for channel in range(3)),
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
    materials = settings.materials
    if not materials:
        raise ValueError("add at least one Painted Material")
    if len(materials) > 0xFFFF:
        raise ValueError("too many Painted Materials")
    material_table = bytearray()
    for material in materials:
        base_block = material.base_block.strip()
        underground_block = material.underground_block.strip()
        if not base_block or not underground_block:
            raise ValueError("material block names cannot be empty")
        base_encoded = base_block.encode("utf-8")
        underground_encoded = underground_block.encode("utf-8")
        if len(base_encoded) > 0xFFFF or len(underground_encoded) > 0xFFFF:
            raise ValueError("Minecraft Block name is too long")
        depth = material.base_depth
        if not isinstance(depth, int) or not 0 < depth <= 0xFFFF:
            raise ValueError("Base Layer Depth must be between 1 and 65535")
        material_table.extend(struct.pack("<H", len(base_encoded)))
        material_table.extend(base_encoded)
        material_table.extend(struct.pack("<H", len(underground_encoded)))
        material_table.extend(underground_encoded)
        material_table.extend(struct.pack("<H", depth))
        if len(material.features) > 0xFFFF:
            raise ValueError("too many surface features")
        material_table.extend(struct.pack("<H", len(material.features)))
        for feature in material.features:
            interval = feature.interval
            if not isinstance(interval, int) or not 0 < interval <= 0xFFFF:
                raise ValueError("Placement Interval must be between 1 and 65535")
            if feature.kind == "SCATTER":
                block = feature.scatter_block.strip()
                encoded = block.encode("utf-8")
                if not block:
                    raise ValueError("Scatter Block cannot be empty")
                if len(encoded) > 0xFFFF:
                    raise ValueError("Scatter Block name is too long")
                material_table.extend(struct.pack("<BH", 1, interval))
                material_table.extend(struct.pack("<H", len(encoded)))
                material_table.extend(encoded)
            else:
                trunk = feature.trunk_block.strip()
                leaves = feature.leaves_block.strip()
                trunk_encoded = trunk.encode("utf-8")
                leaves_encoded = leaves.encode("utf-8")
                if not trunk or not leaves:
                    raise ValueError("Tree trunk and leaves blocks cannot be empty")
                if len(trunk_encoded) > 0xFFFF or len(leaves_encoded) > 0xFFFF:
                    raise ValueError("Tree block name is too long")
                height = feature.tree_height
                radius = feature.canopy_radius
                if not isinstance(height, int) or not 0 < height <= 0xFFFF or not isinstance(radius, int) or radius > 0xFFFF:
                    raise ValueError("Tree dimensions are invalid")
                material_table.extend(struct.pack("<BH", 2, interval))
                material_table.extend(struct.pack("<H", len(trunk_encoded)))
                material_table.extend(trunk_encoded)
                material_table.extend(struct.pack("<H", len(leaves_encoded)))
                material_table.extend(leaves_encoded)
                material_table.extend(struct.pack("<HH", height, radius))

    positions = array("f")
    for vertex in mesh.vertices:
        positions.extend(vertex.co)

    indices = array("I")
    material_ids = array("I")
    for triangle in mesh.loop_triangles:
        indices.extend(triangle.vertices)
        material_ids.append(_nearest_material_id(mesh, mesh.polygons[triangle.polygon_index], materials))

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
        len(materials),
    )
    payload = (
        header
        + material_table
        + _as_little_endian(positions)
        + _as_little_endian(indices)
        + _as_little_endian(material_ids)
    )
    return MeshSnapshot(payload, vertex_count, triangle_count)
