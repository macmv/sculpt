use std::{
  collections::{HashMap, HashSet},
  error::Error,
  fmt,
};

pub const SECTION_EDGE: usize = 16;
pub const SECTION_VOLUME: usize = SECTION_EDGE * SECTION_EDGE * SECTION_EDGE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockPos {
  pub x: i32,
  pub y: i32,
  pub z: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SectionPos {
  pub x: i32,
  pub y: i32,
  pub z: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
  cells: [BlockId; SECTION_VOLUME],
}

impl Section {
  pub fn new(fill: BlockId) -> Self { Self { cells: [fill; SECTION_VOLUME] } }
  pub fn get(&self, x: usize, y: usize, z: usize) -> BlockId { self.cells[cell_index(x, y, z)] }
  pub fn cells(&self) -> &[BlockId; SECTION_VOLUME] { &self.cells }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
  Empty,
  TooLarge,
  EmptyState,
  DuplicateState(String),
  MissingAir,
}

impl fmt::Display for RegistryError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Empty => f.write_str("registry contains no states"),
      Self::TooLarge => f.write_str("registry contains more than 65535 states"),
      Self::EmptyState => f.write_str("registry contains an empty state"),
      Self::DuplicateState(s) => write!(f, "registry contains duplicate state {s:?}"),
      Self::MissingAir => f.write_str("registry does not contain minecraft:air"),
    }
  }
}
impl Error for RegistryError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelloError {
  WrongMagic,
  Truncated,
  TrailingBytes,
  InvalidUtf8,
  Registry(RegistryError),
}
impl fmt::Display for HelloError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::WrongMagic => f.write_str("expected SCLM hello"),
      Self::Truncated => f.write_str("truncated SCLM hello"),
      Self::TrailingBytes => f.write_str("SCLM hello has trailing bytes"),
      Self::InvalidUtf8 => f.write_str("SCLM registry contains invalid UTF-8"),
      Self::Registry(e) => e.fmt(f),
    }
  }
}
impl Error for HelloError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupError(pub String);
impl fmt::Display for LookupError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "unknown block state {:?}", self.0)
  }
}
impl Error for LookupError {}

/// Per-subscriber state. `BlockId`s are meaningful only for this value.
pub struct MinecraftState {
  names:    Vec<String>,
  ids:      HashMap<String, BlockId>,
  air:      BlockId,
  sections: HashMap<SectionPos, Section>,
  modified: HashSet<SectionPos>,
}

impl MinecraftState {
  pub fn new(registry: Vec<String>) -> Result<Self, RegistryError> {
    if registry.is_empty() {
      return Err(RegistryError::Empty);
    }
    if registry.len() > u16::MAX as usize {
      return Err(RegistryError::TooLarge);
    }
    let mut ids = HashMap::with_capacity(registry.len());
    for (index, name) in registry.iter().enumerate() {
      if name.is_empty() {
        return Err(RegistryError::EmptyState);
      }
      if ids.insert(name.clone(), BlockId(index as u16)).is_some() {
        return Err(RegistryError::DuplicateState(name.clone()));
      }
    }
    let air = *ids.get("minecraft:air").ok_or(RegistryError::MissingAir)?;
    Ok(Self { names: registry, ids, air, sections: HashMap::new(), modified: HashSet::new() })
  }

  pub fn lookup(&self, state: &str) -> Result<BlockId, LookupError> {
    self.ids.get(state).copied().ok_or_else(|| LookupError(state.into()))
  }
  pub fn air(&self) -> BlockId { self.air }
  pub fn state_name(&self, id: BlockId) -> Option<&str> {
    self.names.get(id.0 as usize).map(String::as_str)
  }
  pub fn get(&self, position: BlockPos) -> BlockId {
    let (section, x, y, z) = split_position(position);
    self.sections.get(&section).map_or(self.air, |s| s.get(x, y, z))
  }
  /// Returns true only if a cell actually changed. Missing sections are
  /// retained after any write.
  pub fn set(&mut self, position: BlockPos, value: BlockId) -> bool {
    assert!(self.state_name(value).is_some(), "BlockId does not belong to this MinecraftState");
    let (section_pos, x, y, z) = split_position(position);
    let section = self.sections.entry(section_pos).or_insert_with(|| Section::new(self.air));
    let cell = cell_index(x, y, z);
    if section.cells[cell] == value {
      return false;
    }
    section.cells[cell] = value;
    self.modified.insert(section_pos);
    true
  }
  pub fn section(&self, position: SectionPos) -> Option<&Section> { self.sections.get(&position) }
  pub fn drain_modified_sections(&mut self) -> impl Iterator<Item = SectionPos> + '_ {
    self.modified.drain()
  }
  pub fn serialize_section_delta(&self, revision: u64, position: SectionPos) -> Vec<u8> {
    serialize_section_delta(revision, position, self.sections.get(&position), self.air, &self.names)
  }
}

/// Parses the complete, registry-bearing `SCLM` payload.
pub fn parse_hello(payload: &[u8]) -> Result<MinecraftState, HelloError> {
  if payload.len() < 6 {
    return Err(HelloError::Truncated);
  }
  if &payload[..4] != b"SCLM" {
    return Err(HelloError::WrongMagic);
  }
  let count = u16::from_le_bytes([payload[4], payload[5]]) as usize;
  let mut offset = 6;
  let mut registry = Vec::with_capacity(count);
  for _ in 0..count {
    if payload.len() < offset + 2 {
      return Err(HelloError::Truncated);
    }
    let length = u16::from_le_bytes([payload[offset], payload[offset + 1]]) as usize;
    offset += 2;
    let bytes = payload.get(offset..offset + length).ok_or(HelloError::Truncated)?;
    let state = std::str::from_utf8(bytes).map_err(|_| HelloError::InvalidUtf8)?;
    registry.push(state.to_owned());
    offset += length;
  }
  if offset != payload.len() {
    return Err(HelloError::TrailingBytes);
  }
  MinecraftState::new(registry).map_err(HelloError::Registry)
}

pub fn welcome() -> [u8; 8] { *b"SCLW\0\0\0\0" }

/// The connection-owned subscriber state and its bounded, latest-only outgoing
/// queue. The caller writes `welcome()` immediately after `new` succeeds, then
/// drains this queue without ever blocking producers.
pub struct MinecraftSubscriber {
  state:    MinecraftState,
  pending:  HashMap<SectionPos, Vec<u8>>,
  capacity: usize,
}

impl MinecraftSubscriber {
  pub fn new(hello_payload: &[u8], capacity: usize) -> Result<Self, HelloError> {
    Ok(Self { state: parse_hello(hello_payload)?, pending: HashMap::new(), capacity })
  }
  pub fn state(&self) -> &MinecraftState { &self.state }
  pub fn state_mut(&mut self) -> &mut MinecraftState { &mut self.state }
  /// Replaces a queued update for a section. If full, an arbitrary older queued
  /// section is evicted; each retained section always has its newest
  /// replacement.
  pub fn enqueue(&mut self, revision: u64, position: SectionPos) {
    if !self.pending.contains_key(&position) && self.pending.len() == self.capacity {
      if let Some(oldest) = self.pending.keys().next().copied() {
        self.pending.remove(&oldest);
      }
    }
    if self.capacity != 0 {
      self.pending.insert(position, self.state.serialize_section_delta(revision, position));
    }
  }
  pub fn enqueue_modified_sections(&mut self, revision: u64) {
    let positions: Vec<_> = self.state.drain_modified_sections().collect();
    for position in positions {
      self.enqueue(revision, position);
    }
  }
  pub fn take_pending(&mut self) -> impl Iterator<Item = (SectionPos, Vec<u8>)> + '_ {
    self.pending.drain()
  }
}

fn split_position(position: BlockPos) -> (SectionPos, usize, usize, usize) {
  let section = SectionPos {
    x: position.x.div_euclid(16),
    y: position.y.div_euclid(16),
    z: position.z.div_euclid(16),
  };
  (
    section,
    position.x.rem_euclid(16) as usize,
    position.y.rem_euclid(16) as usize,
    position.z.rem_euclid(16) as usize,
  )
}
fn cell_index(x: usize, y: usize, z: usize) -> usize {
  assert!(x < 16 && y < 16 && z < 16);
  x + 16 * (z + 16 * y)
}

/// Encodes a complete section. `None` deliberately becomes a singleton-air
/// replacement.
pub fn serialize_section_delta(
  revision: u64,
  position: SectionPos,
  section: Option<&Section>,
  air: BlockId,
  names: &[String],
) -> Vec<u8> {
  let air_section;
  let cells = match section {
    Some(section) => section.cells(),
    None => {
      air_section = Section::new(air);
      air_section.cells()
    }
  };
  let mut palette = Vec::<BlockId>::new();
  let mut indices = [0u16; SECTION_VOLUME];
  for (cell, &id) in cells.iter().enumerate() {
    let index = palette.iter().position(|&known| known == id).unwrap_or_else(|| {
      palette.push(id);
      palette.len() - 1
    });
    indices[cell] = index as u16;
  }
  let bits = if palette.len() == 1 {
    0
  } else {
    (usize::BITS - (palette.len() - 1).leading_zeros()) as usize
  };
  let words = if bits == 0 { 0 } else { (SECTION_VOLUME * bits).div_ceil(64) };
  let mut output = Vec::new();
  output.extend_from_slice(b"SCLD");
  output.extend_from_slice(&revision.to_le_bytes());
  for coordinate in [position.x, position.y, position.z] {
    output.extend_from_slice(&coordinate.to_le_bytes());
  }
  output.extend_from_slice(&(palette.len() as u16).to_le_bytes());
  for id in palette {
    let name = &names[id.0 as usize];
    output.extend_from_slice(&(name.len() as u16).to_le_bytes());
    output.extend_from_slice(name.as_bytes());
  }
  output.push(bits as u8);
  output.extend_from_slice(&(words as u16).to_le_bytes());
  let mut packed = vec![0u64; words];
  for (cell, &index) in indices.iter().enumerate() {
    if bits == 0 {
      break;
    }
    let offset = cell * bits;
    let word = offset / 64;
    let shift = offset % 64;
    packed[word] |= (index as u64) << shift;
    if shift + bits > 64 {
      packed[word + 1] |= (index as u64) >> (64 - shift);
    }
  }
  for word in packed {
    output.extend_from_slice(&word.to_le_bytes());
  }
  output
}

#[cfg(test)]
mod tests {
  use super::*;
  fn state() -> MinecraftState {
    MinecraftState::new(vec![
      "minecraft:air".into(),
      "minecraft:stone".into(),
      "minecraft:dirt".into(),
    ])
    .unwrap()
  }
  fn hello(names: &[&str]) -> Vec<u8> {
    let mut v = b"SCLM".to_vec();
    v.extend_from_slice(&(names.len() as u16).to_le_bytes());
    for name in names {
      v.extend_from_slice(&(name.len() as u16).to_le_bytes());
      v.extend_from_slice(name.as_bytes());
    }
    v
  }
  #[test]
  fn hello_is_strict() {
    assert_eq!(parse_hello(&hello(&["minecraft:air"])).unwrap().air(), BlockId(0));
    assert!(parse_hello(b"SCLR\0\0").is_err());
    assert!(parse_hello(&hello(&[])).is_err());
    assert!(parse_hello(&hello(&["minecraft:air", "minecraft:air"])).is_err());
    let mut bad = hello(&["minecraft:air"]);
    bad.push(0);
    assert!(matches!(parse_hello(&bad), Err(HelloError::TrailingBytes)));
  }
  #[test]
  fn sparse_state_and_negative_coordinates() {
    let mut s = state();
    let p = BlockPos { x: -1, y: -16, z: -17 };
    assert_eq!(s.get(p), s.air());
    assert!(s.section(SectionPos { x: -1, y: -1, z: -2 }).is_none());
    let stone = s.lookup("minecraft:stone").unwrap();
    assert!(s.set(p, stone));
    assert!(!s.set(p, stone));
    assert_eq!(s.get(p), stone);
    assert_eq!(
      s.drain_modified_sections().collect::<Vec<_>>(),
      vec![SectionPos { x: -1, y: -1, z: -2 }]
    );
    assert!(s.drain_modified_sections().next().is_none());
  }
  #[test]
  fn section_order_and_delta() {
    let mut s = state();
    let stone = s.lookup("minecraft:stone").unwrap();
    s.set(BlockPos { x: 1, y: 2, z: 3 }, stone);
    let bytes = s.serialize_section_delta(9, SectionPos { x: 0, y: 0, z: 0 });
    assert_eq!(&bytes[..4], b"SCLD");
    let palette_start = 24;
    assert_eq!(u16::from_le_bytes(bytes[palette_start..palette_start + 2].try_into().unwrap()), 2);
    let section = s.section(SectionPos { x: 0, y: 0, z: 0 }).unwrap();
    assert_eq!(section.get(1, 2, 3), stone);
    let absent = s.serialize_section_delta(1, SectionPos { x: 9, y: 9, z: 9 });
    assert_eq!(u16::from_le_bytes(absent[24..26].try_into().unwrap()), 1);
  }
}
