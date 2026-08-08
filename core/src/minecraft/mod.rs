mod section;

pub use section::Section;
use std::{
  collections::{HashMap, HashSet},
  error::Error,
  fmt,
};

pub const SECTION_EDGE: usize = 16;
pub const SECTION_VOLUME: usize = SECTION_EDGE * SECTION_EDGE * SECTION_EDGE;
pub const MAX_GLOBAL_BLOCK_STATES: usize = 1 << 15;

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
      Self::TooLarge => {
        f.write_str("registry exceeds Minecraft's 15-bit global block-state palette")
      }
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
    if registry.len() > MAX_GLOBAL_BLOCK_STATES {
      return Err(RegistryError::TooLarge);
    }
    let mut ids = HashMap::with_capacity(registry.len());
    for (i, name) in registry.iter().enumerate() {
      if name.is_empty() {
        return Err(RegistryError::EmptyState);
      }
      if ids.insert(name.clone(), BlockId(i as u16)).is_some() {
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
  pub fn get(&self, p: BlockPos) -> BlockId {
    let (s, x, y, z) = split_position(p);
    self.sections.get(&s).map_or(self.air, |v| v.get(x, y, z))
  }
  pub fn set(&mut self, p: BlockPos, value: BlockId) -> bool {
    assert!(self.state_name(value).is_some(), "BlockId does not belong to this MinecraftState");
    let (pos, x, y, z) = split_position(p);
    let s = self.sections.entry(pos).or_insert_with(|| Section::new(self.air));
    if s.get(x, y, z) == value {
      return false;
    }
    s.set(x, y, z, value);
    self.modified.insert(pos);
    true
  }
  pub fn section(&self, p: SectionPos) -> Option<&Section> { self.sections.get(&p) }
  pub fn drain_modified_sections(&mut self) -> impl Iterator<Item = SectionPos> + '_ {
    self.modified.drain()
  }
  pub fn serialize_section_delta(&self, revision: u64, position: SectionPos) -> Vec<u8> {
    serialize_section_delta(revision, position, self.sections.get(&position), self.air)
  }
}
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
    let len = u16::from_le_bytes([payload[offset], payload[offset + 1]]) as usize;
    offset += 2;
    let bytes = payload.get(offset..offset + len).ok_or(HelloError::Truncated)?;
    registry.push(std::str::from_utf8(bytes).map_err(|_| HelloError::InvalidUtf8)?.to_owned());
    offset += len;
  }
  if offset != payload.len() {
    return Err(HelloError::TrailingBytes);
  }
  MinecraftState::new(registry).map_err(HelloError::Registry)
}
pub fn welcome() -> [u8; 8] { *b"SCLW\0\0\0\0" }
pub struct MinecraftSubscriber {
  state:    MinecraftState,
  pending:  HashMap<SectionPos, Vec<u8>>,
  capacity: usize,
}
impl MinecraftSubscriber {
  pub fn new(h: &[u8], capacity: usize) -> Result<Self, HelloError> {
    Ok(Self { state: parse_hello(h)?, pending: HashMap::new(), capacity })
  }
  pub fn state(&self) -> &MinecraftState { &self.state }
  pub fn state_mut(&mut self) -> &mut MinecraftState { &mut self.state }
  pub fn enqueue(&mut self, revision: u64, pos: SectionPos) {
    if !self.pending.contains_key(&pos) && self.pending.len() == self.capacity {
      if let Some(old) = self.pending.keys().next().copied() {
        self.pending.remove(&old);
      }
    }
    if self.capacity != 0 {
      self.pending.insert(pos, self.state.serialize_section_delta(revision, pos));
    }
  }
  pub fn enqueue_modified_sections(&mut self, revision: u64) {
    let p: Vec<_> = self.state.drain_modified_sections().collect();
    for pos in p {
      self.enqueue(revision, pos)
    }
  }
  pub fn take_pending(&mut self) -> impl Iterator<Item = (SectionPos, Vec<u8>)> + '_ {
    self.pending.drain()
  }
}
fn split_position(p: BlockPos) -> (SectionPos, usize, usize, usize) {
  let s = SectionPos { x: p.x.div_euclid(16), y: p.y.div_euclid(16), z: p.z.div_euclid(16) };
  (s, p.x.rem_euclid(16) as usize, p.y.rem_euclid(16) as usize, p.z.rem_euclid(16) as usize)
}
/// `SCLD` has a little-endian header and the remaining bytes are the exact
/// vanilla section payload.
pub fn serialize_section_delta(
  revision: u64,
  position: SectionPos,
  section: Option<&Section>,
  air: BlockId,
) -> Vec<u8> {
  let fallback;
  let s = match section {
    Some(s) => s,
    None => {
      fallback = Section::new(air);
      &fallback
    }
  };
  let mut o = Vec::new();
  o.extend_from_slice(b"SCLD");
  o.extend_from_slice(&revision.to_le_bytes());
  for v in [position.x, position.y, position.z] {
    o.extend_from_slice(&v.to_le_bytes())
  }
  s.write_network(&mut o, air);
  o
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
    for n in names {
      v.extend_from_slice(&(n.len() as u16).to_le_bytes());
      v.extend_from_slice(n.as_bytes())
    }
    v
  }
  #[test]
  fn hello_is_strict() {
    assert_eq!(parse_hello(&hello(&["minecraft:air"])).unwrap().air(), BlockId(0));
    assert!(parse_hello(&hello(&[])).is_err());
    assert!(parse_hello(&hello(&["minecraft:air", "minecraft:air"])).is_err());
  }
  #[test]
  fn order_and_header() {
    let mut s = state();
    let stone = s.lookup("minecraft:stone").unwrap();
    s.set(BlockPos { x: 1, y: 2, z: 3 }, stone);
    let b = s.serialize_section_delta(9, SectionPos { x: 0, y: 0, z: 0 });
    assert_eq!(&b[..4], b"SCLD");
    assert_eq!(u16::from_be_bytes(b[24..26].try_into().unwrap()), 1);
    assert_eq!(b[26], 4);
    assert_eq!(s.section(SectionPos { x: 0, y: 0, z: 0 }).unwrap().get(1, 2, 3), stone);
  }
}
