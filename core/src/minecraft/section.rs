use super::{BlockId, SECTION_VOLUME};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Storage {
  Singleton(BlockId),
  Indirect { bits: u8, palette: Vec<BlockId>, data: Vec<u64> },
  Direct { data: Vec<u64> },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
  storage: Storage,
}
impl Section {
  pub fn new(fill: BlockId) -> Self { Self { storage: Storage::Singleton(fill) } }
  pub fn get(&self, x: usize, y: usize, z: usize) -> BlockId {
    let i = index(x, y, z);
    match &self.storage {
      Storage::Singleton(v) => *v,
      Storage::Indirect { bits, palette, data } => palette[read(data, *bits, i) as usize],
      Storage::Direct { data } => BlockId(read(data, 15, i) as u16),
    }
  }
  pub fn set(&mut self, x: usize, y: usize, z: usize, value: BlockId) {
    let i = index(x, y, z);
    match &mut self.storage {
      Storage::Singleton(old) if *old == value => return,
      Storage::Singleton(old) => {
        let p = vec![*old, value];
        self.storage = Storage::Indirect {
          bits:    4,
          data:    pack(4, SECTION_VOLUME, |n| if n == i { 1 } else { 0 }),
          palette: p,
        };
      }
      Storage::Indirect { bits, palette, data } => {
        if let Some(p) = palette.iter().position(|&v| v == value) {
          write(data, *bits, i, p as u64);
          return;
        }
        if palette.len() < (1usize << *bits) {
          palette.push(value);
          write(data, *bits, i, (palette.len() - 1) as u64);
          return;
        }
        if *bits < 8 {
          let next = *bits + 1;
          let old = std::mem::take(data);
          *data = pack(next, SECTION_VOLUME, |n| read(&old, *bits, n));
          *bits = next;
          palette.push(value);
          write(data, *bits, i, (palette.len() - 1) as u64);
          return;
        }
        let old = std::mem::take(data);
        let old_bits = *bits;
        let old_palette = std::mem::take(palette);
        let mut direct =
          pack(15, SECTION_VOLUME, |n| old_palette[read(&old, old_bits, n) as usize].0 as u64);
        write(&mut direct, 15, i, value.0 as u64);
        self.storage = Storage::Direct { data: direct };
      }
      Storage::Direct { data } => write(data, 15, i, value.0 as u64),
    }
  }
  pub fn non_air_count(&self, air: BlockId) -> u16 {
    (0..SECTION_VOLUME).filter(|&i| self.at(i) != air).count() as u16
  }
  fn at(&self, i: usize) -> BlockId { self.get(i & 15, i >> 8, (i >> 4) & 15) }
  /// The block-state prefix of vanilla `LevelChunkSection.write`: big-endian
  /// non-air and fluid counts followed by the block-state `PalettedContainer`.
  /// The receiver appends its existing biome container before invoking the
  /// native section reader, since Sculpt does not transport biome changes.
  pub fn write_network(&self, out: &mut Vec<u8>, air: BlockId) {
    out.extend_from_slice(&self.non_air_count(air).to_be_bytes());
    // Core-generated sections currently contain only air and solid blocks.
    // The native reader still requires this counter even when it is zero.
    out.extend_from_slice(&0u16.to_be_bytes());
    match &self.storage {
      Storage::Singleton(v) => {
        out.push(0);
        varint(out, v.0 as u32);
        varint(out, 0)
      }
      Storage::Indirect { bits, palette, data } => {
        out.push(*bits);
        varint(out, palette.len() as u32);
        for v in palette {
          varint(out, v.0 as u32)
        }
        write_longs(out, data)
      }
      Storage::Direct { data } => {
        out.push(15);
        write_longs(out, data)
      }
    }
  }
}
fn index(x: usize, y: usize, z: usize) -> usize {
  assert!(x < 16 && y < 16 && z < 16);
  x + 16 * (z + 16 * y)
}
fn words(bits: u8, n: usize) -> usize { n.div_ceil(64 / bits as usize) }
fn read(data: &[u64], bits: u8, i: usize) -> u64 {
  let per = 64 / bits as usize;
  (data[i / per] >> ((i % per) * bits as usize)) & ((1u64 << bits) - 1)
}
fn write(data: &mut [u64], bits: u8, i: usize, v: u64) {
  let per = 64 / bits as usize;
  let shift = (i % per) * bits as usize;
  let mask = ((1u64 << bits) - 1) << shift;
  data[i / per] = (data[i / per] & !mask) | (v << shift)
}
fn pack(bits: u8, n: usize, f: impl Fn(usize) -> u64) -> Vec<u64> {
  let mut d = vec![0; words(bits, n)];
  for i in 0..n {
    write(&mut d, bits, i, f(i));
  }
  d
}
fn varint(out: &mut Vec<u8>, mut v: u32) {
  while v & !0x7f != 0 {
    out.push((v as u8 & 0x7f) | 0x80);
    v >>= 7;
  }
  out.push(v as u8)
}
fn write_longs(out: &mut Vec<u8>, data: &[u64]) {
  varint(out, data.len() as u32);
  for &v in data {
    out.extend_from_slice(&v.to_be_bytes())
  }
}
#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn promotions_and_order() {
    let mut s = Section::new(BlockId(0));
    for n in 1..=17 {
      s.set(n & 15, n >> 8, (n >> 4) & 15, BlockId(n as u16));
    }
    assert!(matches!(s.storage, Storage::Indirect { bits: 5, .. }));
    for n in 18..=257 {
      s.set(n & 15, n >> 8, (n >> 4) & 15, BlockId(n as u16));
    }
    assert!(matches!(s.storage, Storage::Direct { .. }));
    assert_eq!(s.get(1, 0, 0), BlockId(1));
  }
  #[test]
  fn padded_lengths() {
    assert_eq!(words(5, 4096), 342);
    assert_eq!(words(15, 4096), 1024);
  }
}
