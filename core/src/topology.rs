//! Request-local mesh-to-block reconciliation.  Nothing in this module is
//! retained after `reconcile` returns.

use std::{collections::HashMap, error::Error, fmt};

use rand::{RngExt, SeedableRng, rngs::SmallRng};

use crate::{
  blender::{MeshSnapshot, SurfaceFeature},
  minecraft::{BlockId, BlockPos, MinecraftState},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRegion {
  pub min: BlockPos,
  pub max: BlockPos,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopologyError {
  EmptyMesh,
  InvalidRegion,
  NonManifold,
  InconsistentOrientation,
  DegenerateTriangle,
  UnknownMaterial(String),
  CoordinateOverflow,
}

impl fmt::Display for TopologyError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(match self {
      Self::EmptyMesh => "mesh contains no triangles",
      Self::InvalidRegion => "block region minimum exceeds its maximum",
      Self::NonManifold => "mesh is not a watertight manifold",
      Self::InconsistentOrientation => "mesh has inconsistently oriented faces",
      Self::DegenerateTriangle => "mesh contains a degenerate triangle",
      Self::UnknownMaterial(state) => {
        return write!(f, "Minecraft registry does not contain material {state:?}");
      }
      Self::CoordinateOverflow => "mesh scan range exceeds Minecraft coordinate limits",
    })
  }
}
impl Error for TopologyError {}

/// Classifies block centers against a closed, consistently oriented mesh.
/// Validation and BVH construction complete before this function mutates state.
pub fn reconcile(
  snapshot: &MeshSnapshot,
  region: Option<BlockRegion>,
  state: &mut MinecraftState,
) -> Result<(), TopologyError> {
  let mesh = Mesh::from_snapshot(snapshot)?;
  let region = match region {
    Some(region) => validate_region(region)?,
    None => mesh.default_region()?,
  };
  // Resolve every configured state before writing anything so a registry error
  // cannot leave a partially updated terrain region.
  let materials: Vec<_> = snapshot
    .materials
    .iter()
    .map(|material| {
      Ok(ResolvedMaterial {
        base:        state
          .lookup(&material.base_block)
          .map_err(|_| TopologyError::UnknownMaterial(material.base_block.clone()))?,
        underground: state
          .lookup(&material.underground_block)
          .map_err(|_| TopologyError::UnknownMaterial(material.underground_block.clone()))?,
        base_depth:  material.base_depth,
        features:    material
          .features
          .iter()
          .map(|feature| match feature {
            SurfaceFeature::Scatter { block, interval } => Ok(ResolvedFeature::Scatter {
              block:    state
                .lookup(block)
                .map_err(|_| TopologyError::UnknownMaterial(block.clone()))?,
              interval: *interval,
            }),
            SurfaceFeature::Tree { trunk, leaves, interval, height, canopy_radius } => {
              Ok(ResolvedFeature::Tree {
                trunk:         state
                  .lookup(trunk)
                  .map_err(|_| TopologyError::UnknownMaterial(trunk.clone()))?,
                leaves:        state
                  .lookup(leaves)
                  .map_err(|_| TopologyError::UnknownMaterial(leaves.clone()))?,
                interval:      *interval,
                height:        *height,
                canopy_radius: *canopy_radius,
              })
            }
          })
          .collect::<Result<_, _>>()?,
      })
    })
    .collect::<Result<_, _>>()?;
  let air = state.air();
  let bvh = Bvh::new(&mesh.triangles);
  let width = (region.max.x as i64 - region.min.x as i64 + 1) as usize;
  let height = (region.max.y as i64 - region.min.y as i64 + 1) as usize;
  let depth = (region.max.z as i64 - region.min.z as i64 + 1) as usize;
  let mut selected = vec![None; width * height * depth];

  for y in region.min.y..=region.max.y {
    for z in region.min.z..=region.max.z {
      let mut hits = Vec::new();
      bvh.line_hits(&mesh.triangles, y as f64 + 0.5, z as f64 + 0.5, &mut hits);
      hits.sort_by(|a, b| a.0.total_cmp(&b.0));
      // A quad split into two triangles reports the same crossing twice.
      hits.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-8);
      let mut hit = 0;
      let mut layers = Vec::new();
      for x in region.min.x..=region.max.x {
        let center = x as f64 + 0.5;
        while hit < hits.len() && hits[hit].0 < center {
          if hit % 2 == 0 {
            layers.push(mesh.triangles[hits[hit].1].material_id as usize);
          } else {
            layers.pop();
          }
          hit += 1;
        }
        let index = ((y - region.min.y) as usize * depth + (z - region.min.z) as usize) * width
          + (x - region.min.x) as usize;
        selected[index] = layers.last().copied();
      }
    }
  }
  // Layering is vertical in Minecraft coordinates, not along a face normal.
  // That keeps cave walls and side faces from receiving surface blocks.
  for z in 0..depth {
    for x in 0..width {
      let mut top_material = None;
      let mut solid_depth = 0_u16;
      for y in (0..height).rev() {
        let index = (y * depth + z) * width + x;
        let pos = BlockPos {
          x: region.min.x + x as i32,
          y: region.min.y + y as i32,
          z: region.min.z + z as i32,
        };
        match selected[index] {
          Some(material) => {
            let material = if let Some(material) = top_material {
              material
            } else {
              top_material = Some(material);
              material
            };
            let definition = &materials[material];
            let block = if solid_depth < definition.base_depth {
              definition.base
            } else {
              definition.underground
            };
            state.set(pos, block);
            solid_depth = solid_depth.saturating_add(1);
          }
          None => {
            state.set(pos, air);
          }
        }
      }
    }
  }
  apply_surface_features(&materials, &selected, region, width, height, depth, state, air);
  Ok(())
}

#[derive(Clone)]
struct ResolvedMaterial {
  base:        BlockId,
  underground: BlockId,
  base_depth:  u16,
  features:    Vec<ResolvedFeature>,
}

#[derive(Clone, Copy)]
enum ResolvedFeature {
  Scatter {
    block:    BlockId,
    interval: u16,
  },
  Tree {
    trunk:         BlockId,
    leaves:        BlockId,
    interval:      u16,
    height:        u16,
    canopy_radius: u16,
  },
}

fn apply_surface_features(
  materials: &[ResolvedMaterial],
  selected: &[Option<usize>],
  region: BlockRegion,
  width: usize,
  height: usize,
  depth: usize,
  state: &mut MinecraftState,
  air: BlockId,
) {
  for z in 0..depth {
    for x in 0..width {
      let material = (0..height).rev().find_map(|y| selected[(y * depth + z) * width + x]);
      let Some(material) = material else { continue };
      let top_y =
        (0..height).rev().find(|&y| selected[(y * depth + z) * width + x].is_some()).unwrap();
      let x = region.min.x + x as i32;
      let y = region.min.y + top_y as i32 + 1;
      let z = region.min.z + z as i32;
      for feature in &materials[material].features {
        let origin = BlockPos { x, y, z };
        let mut rng = feature_rng(origin);
        match *feature {
          ResolvedFeature::Scatter { block, interval }
            if rng.random_ratio(1, interval as u32) && state.get(origin) == air =>
          {
            state.set(origin, block);
          }
          ResolvedFeature::Tree { trunk, leaves, interval, height, canopy_radius }
            if rng.random_ratio(1, interval as u32) =>
          {
            place_tree(state, air, origin, trunk, leaves, height, canopy_radius, &mut rng)
          }
          _ => {}
        }
      }
    }
  }
}

fn feature_rng(origin: BlockPos) -> SmallRng {
  SmallRng::seed_from_u64(((origin.x << 16) | (origin.y << 8) | origin.z) as u64)
}
fn place_tree(
  state: &mut MinecraftState,
  air: BlockId,
  root: BlockPos,
  trunk: BlockId,
  leaves: BlockId,
  height: u16,
  radius: u16,
  rng: &mut SmallRng,
) {
  if (0..height as i32).any(|dy| state.get(BlockPos { y: root.y + dy, ..root }) != air) {
    return;
  }
  for dy in 0..height as i32 {
    state.set(BlockPos { y: root.y + dy, ..root }, trunk);
  }
  let crown = root.y + height as i32 - 1;
  for dz in -(radius as i32)..=radius as i32 {
    for dx in -(radius as i32)..=radius as i32 {
      if dx.abs() + dz.abs() <= radius as i32
        && state.get(BlockPos { x: root.x + dx, y: crown, z: root.z + dz }) == air
      {
        state.set(BlockPos { x: root.x + dx, y: crown, z: root.z + dz }, leaves);
      }
    }
  }
  // A smaller, randomized upper canopy avoids cloned-looking tree crowns.
  let top = crown + 1;
  let upper_radius = (radius as i32).saturating_sub(1);
  for dz in -upper_radius..=upper_radius {
    for dx in -upper_radius..=upper_radius {
      let pos = BlockPos { x: root.x + dx, y: top, z: root.z + dz };
      if dx.abs() + dz.abs() <= upper_radius
        && (dx == 0 && dz == 0 || rng.random_ratio(2, 3))
        && state.get(pos) == air
      {
        state.set(pos, leaves);
      }
    }
  }
}

fn validate_region(region: BlockRegion) -> Result<BlockRegion, TopologyError> {
  if region.min.x > region.max.x || region.min.y > region.max.y || region.min.z > region.max.z {
    Err(TopologyError::InvalidRegion)
  } else {
    Ok(region)
  }
}

struct Mesh {
  triangles: Vec<Tri>,
  min:       [f64; 3],
  max:       [f64; 3],
}
impl Mesh {
  fn from_snapshot(snapshot: &MeshSnapshot) -> Result<Self, TopologyError> {
    if snapshot.triangles.is_empty() {
      return Err(TopologyError::EmptyMesh);
    }
    let vertices: Vec<_> = snapshot.vertices.iter().map(|&p| map_point(snapshot, p)).collect();
    let mut edges: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
    let mut triangles = Vec::with_capacity(snapshot.triangles.len());
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for face in &snapshot.triangles {
      let [a, b, c] = face.indices;
      if [a, b, c].iter().any(|&i| i as usize >= vertices.len()) {
        return Err(TopologyError::NonManifold);
      }
      if a == b || b == c || c == a {
        return Err(TopologyError::DegenerateTriangle);
      }
      for (from, to) in [(a, b), (b, c), (c, a)] {
        let key = if from < to { (from, to) } else { (to, from) };
        let entry = edges.entry(key).or_insert((0, 0));
        if from < to { entry.0 += 1 } else { entry.1 += 1 }
      }
      let tri = Tri {
        p:           [vertices[a as usize], vertices[b as usize], vertices[c as usize]],
        material_id: face.material_id,
      };
      if tri.area2() < 1e-18 {
        return Err(TopologyError::DegenerateTriangle);
      }
      for p in tri.p {
        for i in 0..3 {
          min[i] = min[i].min(p[i]);
          max[i] = max[i].max(p[i]);
        }
      }
      triangles.push(tri);
    }
    for (_, (forward, backward)) in edges {
      if forward + backward != 2 {
        return Err(TopologyError::NonManifold);
      }
      if forward != 1 || backward != 1 {
        return Err(TopologyError::InconsistentOrientation);
      }
    }
    Ok(Self { triangles, min, max })
  }
  fn default_region(&self) -> Result<BlockRegion, TopologyError> {
    let conv = |v: f64, ceil: bool| -> Result<i32, TopologyError> {
      let n = if ceil { v.ceil() } else { v.floor() };
      if n < i32::MIN as f64 + 1. || n > i32::MAX as f64 - 1. {
        Err(TopologyError::CoordinateOverflow)
      } else {
        Ok(n as i32)
      }
    };
    Ok(BlockRegion {
      min: BlockPos {
        x: conv(self.min[0], false)? - 1,
        y: conv(self.min[1], false)? - 1,
        z: conv(self.min[2], false)? - 1,
      },
      max: BlockPos {
        x: conv(self.max[0], true)? + 1,
        y: conv(self.max[1], true)? + 1,
        z: conv(self.max[2], true)? + 1,
      },
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::blender::{Material, SurfaceFeature, Triangle};

  fn state() -> MinecraftState {
    MinecraftState::new(vec![
      "minecraft:air".into(),
      "minecraft:stone".into(),
      "minecraft:grass_block".into(),
      "minecraft:dirt".into(),
      "minecraft:sand".into(),
      "minecraft:short_grass".into(),
      "minecraft:oak_log".into(),
      "minecraft:oak_leaves".into(),
    ])
    .unwrap()
  }
  fn cube() -> MeshSnapshot {
    // These local coordinates map to Minecraft x/y/z respectively as x/z/-y.
    let desired = [
      [0., 0., 0.],
      [1., 0., 0.],
      [1., 1., 0.],
      [0., 1., 0.],
      [0., 0., 1.],
      [1., 0., 1.],
      [1., 1., 1.],
      [0., 1., 1.],
    ];
    let vertices = desired.into_iter().map(|p| [p[0], -p[2], p[1]]).collect();
    let faces = [
      [0, 2, 1],
      [0, 3, 2],
      [4, 5, 6],
      [4, 6, 7],
      [0, 1, 5],
      [0, 5, 4],
      [3, 7, 6],
      [3, 6, 2],
      [0, 4, 7],
      [0, 7, 3],
      [1, 2, 6],
      [1, 6, 5],
    ];
    MeshSnapshot {
      revision: 1,
      transform: [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.],
      dirty_aabb: [0.; 6],
      units_per_block: 1.,
      minecraft_origin: [0; 3],
      materials: vec![Material {
        base_block:        "minecraft:stone".into(),
        underground_block: "minecraft:stone".into(),
        base_depth:        1,
        features:          vec![],
      }],
      vertices,
      triangles: faces.into_iter().map(|indices| Triangle { indices, material_id: 0 }).collect(),
    }
  }
  #[test]
  fn fills_cube_center_and_clears_exterior() {
    let mut s = state();
    let stone = s.lookup("minecraft:stone").unwrap();
    s.set(BlockPos { x: 2, y: 0, z: 0 }, stone);
    reconcile(
      &cube(),
      Some(BlockRegion {
        min: BlockPos { x: -1, y: -1, z: -1 },
        max: BlockPos { x: 2, y: 2, z: 2 },
      }),
      &mut s,
    )
    .unwrap();
    assert_eq!(s.get(BlockPos { x: 0, y: 0, z: 0 }), stone);
    assert_eq!(s.get(BlockPos { x: 2, y: 0, z: 0 }), s.air());
  }
  #[test]
  fn explicit_region_does_not_write_elsewhere() {
    let mut s = state();
    let stone = s.lookup("minecraft:stone").unwrap();
    s.set(BlockPos { x: 2, y: 0, z: 0 }, stone);
    reconcile(
      &cube(),
      Some(BlockRegion { min: BlockPos { x: 0, y: 0, z: 0 }, max: BlockPos { x: 0, y: 0, z: 0 } }),
      &mut s,
    )
    .unwrap();
    assert_eq!(s.get(BlockPos { x: 0, y: 0, z: 0 }), stone);
    assert_eq!(s.get(BlockPos { x: 2, y: 0, z: 0 }), stone);
  }
  #[test]
  fn invalid_mesh_does_not_modify_state() {
    let mut s = state();
    let stone = s.lookup("minecraft:stone").unwrap();
    s.set(BlockPos { x: 4, y: 4, z: 4 }, stone);
    let mut mesh = cube();
    mesh.triangles.pop();
    assert!(reconcile(&mesh, None, &mut s).is_err());
    assert_eq!(s.get(BlockPos { x: 4, y: 4, z: 4 }), stone);
  }

  #[test]
  fn layers_each_column_using_its_topmost_material() {
    let mut s = state();
    let mut mesh = cube();
    // Make this a four-block-tall cube and paint its top face grass-like.
    for vertex in &mut mesh.vertices {
      if vertex[2] == 1. {
        vertex[2] = 4.;
      }
    }
    mesh.materials = vec![
      Material {
        base_block:        "minecraft:grass_block".into(),
        underground_block: "minecraft:dirt".into(),
        base_depth:        2,
        features:          vec![SurfaceFeature::Scatter {
          block:    "minecraft:short_grass".into(),
          interval: 1,
        }],
      },
      Material {
        base_block:        "minecraft:sand".into(),
        underground_block: "minecraft:stone".into(),
        base_depth:        1,
        features:          vec![SurfaceFeature::Tree {
          trunk:         "minecraft:oak_log".into(),
          leaves:        "minecraft:oak_leaves".into(),
          interval:      1,
          height:        3,
          canopy_radius: 1,
        }],
      },
    ];
    // The x=0 side is the entering surface for the +X classification ray.
    mesh.triangles[8].material_id = 0;
    mesh.triangles[9].material_id = 0;
    let mut adjacent = mesh.clone();
    for vertex in &mut adjacent.vertices {
      vertex[0] += 2.;
    }
    let vertex_offset = mesh.vertices.len() as u32;
    mesh.vertices.extend(adjacent.vertices);
    mesh.triangles.extend(adjacent.triangles.into_iter().map(|mut triangle| {
      triangle.indices = triangle.indices.map(|index| index + vertex_offset);
      triangle.material_id = if triangle.material_id == 0 { 1 } else { triangle.material_id };
      triangle
    }));
    reconcile(
      &mesh,
      Some(BlockRegion { min: BlockPos { x: 0, y: 0, z: 0 }, max: BlockPos { x: 2, y: 3, z: 0 } }),
      &mut s,
    )
    .unwrap();
    let grass = s.lookup("minecraft:grass_block").unwrap();
    let dirt = s.lookup("minecraft:dirt").unwrap();
    let sand = s.lookup("minecraft:sand").unwrap();
    let stone = s.lookup("minecraft:stone").unwrap();
    let short_grass = s.lookup("minecraft:short_grass").unwrap();
    let oak_log = s.lookup("minecraft:oak_log").unwrap();
    let oak_leaves = s.lookup("minecraft:oak_leaves").unwrap();
    assert_eq!(s.get(BlockPos { x: 0, y: 3, z: 0 }), grass);
    assert_eq!(s.get(BlockPos { x: 0, y: 2, z: 0 }), grass);
    assert_eq!(s.get(BlockPos { x: 0, y: 1, z: 0 }), dirt);
    assert_eq!(s.get(BlockPos { x: 2, y: 3, z: 0 }), sand);
    assert_eq!(s.get(BlockPos { x: 2, y: 2, z: 0 }), stone);
    assert_eq!(s.get(BlockPos { x: 0, y: 4, z: 0 }), short_grass);
    assert_eq!(s.get(BlockPos { x: 2, y: 4, z: 0 }), oak_log);
    assert_eq!(s.get(BlockPos { x: 2, y: 7, z: 0 }), oak_leaves);
  }
}

fn map_point(s: &MeshSnapshot, p: [f32; 3]) -> [f64; 3] {
  let m = &s.transform;
  let x = p[0];
  let y = p[1];
  let z = p[2];
  let wx = m[0] * x + m[1] * y + m[2] * z + m[3];
  let wy = m[4] * x + m[5] * y + m[6] * z + m[7];
  let wz = m[8] * x + m[9] * y + m[10] * z + m[11];
  let w = m[12] * x + m[13] * y + m[14] * z + m[15];
  let (wx, wy, wz) = if w != 0. { (wx / w, wy / w, wz / w) } else { (wx, wy, wz) };
  [
    s.minecraft_origin[0] as f64 + wx as f64 / s.units_per_block as f64,
    s.minecraft_origin[1] as f64 + wz as f64 / s.units_per_block as f64,
    s.minecraft_origin[2] as f64 - wy as f64 / s.units_per_block as f64,
  ]
}

#[derive(Clone, Copy)]
struct Tri {
  p:           [[f64; 3]; 3],
  material_id: u32,
}
impl Tri {
  fn area2(&self) -> f64 {
    let u = sub(self.p[1], self.p[0]);
    let v = sub(self.p[2], self.p[0]);
    let c = cross(u, v);
    dot(c, c)
  }
  fn bounds(&self) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in self.p {
      for i in 0..3 {
        lo[i] = lo[i].min(p[i]);
        hi[i] = hi[i].max(p[i]);
      }
    }
    (lo, hi)
  }
  fn line_hit(&self, y: f64, z: f64) -> Option<f64> {
    // Moller-Trumbore, ray +X from negative infinity
    let e1 = sub(self.p[1], self.p[0]);
    let e2 = sub(self.p[2], self.p[0]);
    let d = [1., 0., 0.];
    let h = cross(d, e2);
    let a = dot(e1, h);
    if a.abs() < 1e-12 {
      return None;
    };
    let f = 1. / a;
    let q0 = [0., y, z];
    let s = sub(q0, self.p[0]);
    let u = f * dot(s, h);
    if !(0. ..=1.).contains(&u) {
      return None;
    };
    let q = cross(s, e1);
    let v = f * dot(d, q);
    if v < 0. || u + v > 1. {
      return None;
    };
    Some(f * dot(e2, q))
  }
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
  [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

struct Bvh {
  nodes:   Vec<Node>,
  indices: Vec<usize>,
}
struct Node {
  lo:       [f64; 3],
  hi:       [f64; 3],
  children: Option<(usize, usize)>,
  range:    std::ops::Range<usize>,
}
impl Bvh {
  fn new(tris: &[Tri]) -> Self {
    let mut b = Self { nodes: Vec::new(), indices: (0..tris.len()).collect() };
    b.build(tris, 0, tris.len());
    b
  }
  fn build(&mut self, tris: &[Tri], start: usize, end: usize) -> usize {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for &i in &self.indices[start..end] {
      let (a, b) = tris[i].bounds();
      for n in 0..3 {
        lo[n] = lo[n].min(a[n]);
        hi[n] = hi[n].max(b[n]);
      }
    }
    let me = self.nodes.len();
    self.nodes.push(Node { lo, hi, children: None, range: start..end });
    if end - start > 8 {
      let axis = (0..3).max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b]))).unwrap();
      self.indices[start..end].sort_by(|&a, &b| {
        let aa = tris[a].bounds();
        let bb = tris[b].bounds();
        ((aa.0[axis] + aa.1[axis]) * 0.5).total_cmp(&((bb.0[axis] + bb.1[axis]) * 0.5))
      });
      let mid = (start + end) / 2;
      let l = self.build(tris, start, mid);
      let r = self.build(tris, mid, end);
      self.nodes[me].children = Some((l, r));
    }
    me
  }
  fn line_hits(&self, tris: &[Tri], y: f64, z: f64, out: &mut Vec<(f64, usize)>) {
    self.visit(0, tris, y, z, out)
  }
  fn visit(&self, n: usize, tris: &[Tri], y: f64, z: f64, out: &mut Vec<(f64, usize)>) {
    let node = &self.nodes[n];
    if y < node.lo[1] || y > node.hi[1] || z < node.lo[2] || z > node.hi[2] {
      return;
    }
    if let Some((l, r)) = node.children {
      self.visit(l, tris, y, z, out);
      self.visit(r, tris, y, z, out)
    } else {
      for &i in &self.indices[node.range.clone()] {
        if let Some(x) = tris[i].line_hit(y, z) {
          out.push((x, i))
        }
      }
    }
  }
}
