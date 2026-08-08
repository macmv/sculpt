use std::{error::Error, fmt, io::Read};

const MAGIC: [u8; 4] = *b"SCLP";
const VERSION: u16 = 1;
const HEADER_SIZE: usize = 132;
const FULL_SNAPSHOT_FLAG: u32 = 1;
const MAX_FRAME_SIZE: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct MeshSnapshot {
  pub object_id:  [u8; 16],
  pub revision:   u64,
  pub transform:  [f32; 16],
  pub dirty_aabb: [f32; 6],
  pub vertices:   Vec<[f32; 3]>,
  pub triangles:  Vec<Triangle>,
}

#[derive(Debug, Clone)]
pub struct Triangle {
  pub indices:     [u32; 3],
  pub material_id: u32,
}

#[derive(Debug)]
pub enum ReadError {
  Io(std::io::Error),
  Message(MessageError),
}

impl fmt::Display for ReadError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Io(error) => write!(formatter, "socket read failed: {error}"),
      Self::Message(error) => error.fmt(formatter),
    }
  }
}

impl Error for ReadError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageError(String);

impl fmt::Display for MessageError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { formatter.write_str(&self.0) }
}

impl Error for MessageError {}

/// Reads one length-prefixed Blender mesh snapshot from the Unix stream.
pub fn read_mesh_snapshot(reader: &mut impl Read) -> Result<MeshSnapshot, ReadError> {
  let mut length_bytes = [0; 4];
  reader.read_exact(&mut length_bytes).map_err(ReadError::Io)?;
  let length = u32::from_le_bytes(length_bytes) as usize;

  if length > MAX_FRAME_SIZE {
    return Err(message_error(format!(
      "frame length {length} exceeds the {MAX_FRAME_SIZE}-byte limit"
    )));
  }

  let mut body = vec![0; length];
  reader.read_exact(&mut body).map_err(ReadError::Io)?;
  parse_mesh_snapshot(&body).map_err(ReadError::Message)
}

/// Parses the body of one framed Blender mesh snapshot.
pub fn parse_mesh_snapshot(body: &[u8]) -> Result<MeshSnapshot, MessageError> {
  if body.len() < HEADER_SIZE {
    return Err(MessageError("message is shorter than the fixed header".into()));
  }
  if body[0..4] != MAGIC {
    return Err(MessageError("incorrect message magic".into()));
  }
  if read_u16(body, 4) != VERSION {
    return Err(MessageError("unsupported protocol version".into()));
  }
  if read_u16(body, 6) as usize != HEADER_SIZE {
    return Err(MessageError("incorrect header size".into()));
  }
  if read_u32(body, 32) != FULL_SNAPSHOT_FLAG {
    return Err(MessageError("message is not a full mesh snapshot".into()));
  }

  let vertex_count = read_u32(body, 36) as usize;
  let triangle_count = read_u32(body, 40) as usize;
  let vertex_bytes = vertex_count
    .checked_mul(3 * size_of::<f32>())
    .ok_or_else(|| MessageError("vertex buffer length overflows".into()))?;
  let index_bytes = triangle_count
    .checked_mul(3 * size_of::<u32>())
    .ok_or_else(|| MessageError("triangle index buffer length overflows".into()))?;
  let material_bytes = triangle_count
    .checked_mul(size_of::<u32>())
    .ok_or_else(|| MessageError("material buffer length overflows".into()))?;
  let expected_length = HEADER_SIZE
    .checked_add(vertex_bytes)
    .and_then(|length| length.checked_add(index_bytes))
    .and_then(|length| length.checked_add(material_bytes))
    .ok_or_else(|| MessageError("message length overflows".into()))?;

  if body.len() != expected_length {
    return Err(MessageError(format!(
      "message length is {}, expected {expected_length}",
      body.len()
    )));
  }

  let object_id = body[8..24].try_into().expect("fixed-width object ID");
  let revision = read_u64(body, 24);
  let transform = read_f32_array::<16>(body, 44);
  let dirty_aabb = read_f32_array::<6>(body, 108);
  validate_finite("transform", &transform)?;
  validate_finite("dirty AABB", &dirty_aabb)?;
  if dirty_aabb[0] > dirty_aabb[3] || dirty_aabb[1] > dirty_aabb[4] || dirty_aabb[2] > dirty_aabb[5]
  {
    return Err(MessageError("dirty AABB minimum exceeds its maximum".into()));
  }

  let mut offset = HEADER_SIZE;
  let mut vertices = Vec::with_capacity(vertex_count);
  for _ in 0..vertex_count {
    let vertex = read_f32_array::<3>(body, offset);
    validate_finite("vertex position", &vertex)?;
    vertices.push(vertex);
    offset += 3 * size_of::<f32>();
  }

  let mut indices = Vec::with_capacity(triangle_count);
  for _ in 0..triangle_count {
    let triangle = read_u32_array::<3>(body, offset);
    if triangle.iter().any(|&index| index as usize >= vertex_count) {
      return Err(MessageError("triangle references an out-of-range vertex".into()));
    }
    indices.push(triangle);
    offset += 3 * size_of::<u32>();
  }

  let triangles = indices
    .into_iter()
    .map(|indices| {
      let material_id = read_u32(body, offset);
      offset += size_of::<u32>();
      Triangle { indices, material_id }
    })
    .collect();

  Ok(MeshSnapshot { object_id, revision, transform, dirty_aabb, vertices, triangles })
}

fn message_error(message: String) -> ReadError { ReadError::Message(MessageError(message)) }

fn validate_finite(name: &str, values: &[f32]) -> Result<(), MessageError> {
  if values.iter().any(|value| !value.is_finite()) {
    return Err(MessageError(format!("{name} contains a non-finite value")));
  }
  Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
  u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("validated header"))
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
  u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("validated message"))
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
  u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("validated header"))
}

fn read_f32_array<const N: usize>(bytes: &[u8], offset: usize) -> [f32; N] {
  std::array::from_fn(|index| read_f32(bytes, offset + index * size_of::<f32>()))
}

fn read_u32_array<const N: usize>(bytes: &[u8], offset: usize) -> [u32; N] {
  std::array::from_fn(|index| read_u32(bytes, offset + index * size_of::<u32>()))
}

fn read_f32(bytes: &[u8], offset: usize) -> f32 {
  f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("validated message"))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn valid_message() -> Vec<u8> {
    let mut message = vec![0; HEADER_SIZE];
    message[0..4].copy_from_slice(&MAGIC);
    message[4..6].copy_from_slice(&VERSION.to_le_bytes());
    message[6..8].copy_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
    message[8..24].copy_from_slice(&[42; 16]);
    message[24..32].copy_from_slice(&7_u64.to_le_bytes());
    message[32..36].copy_from_slice(&FULL_SNAPSHOT_FLAG.to_le_bytes());
    message[36..40].copy_from_slice(&3_u32.to_le_bytes());
    message[40..44].copy_from_slice(&1_u32.to_le_bytes());
    for index in 0..16 {
      message[44 + index * 4..48 + index * 4].copy_from_slice(&(index as f32).to_le_bytes());
    }
    for (index, value) in [0.0_f32, 1.0, 2.0, 3.0, 4.0, 5.0].into_iter().enumerate() {
      message[108 + index * 4..112 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
      message.extend_from_slice(&value.to_le_bytes());
    }
    for index in [0_u32, 1, 2] {
      message.extend_from_slice(&index.to_le_bytes());
    }
    message.extend_from_slice(&4_u32.to_le_bytes());
    message
  }

  #[test]
  fn parses_a_valid_mesh_snapshot() {
    let mesh = parse_mesh_snapshot(&valid_message()).unwrap();
    assert_eq!(mesh.object_id, [42; 16]);
    assert_eq!(mesh.revision, 7);
    assert_eq!(mesh.vertices.len(), 3);
    assert_eq!(mesh.triangles.len(), 1);
    assert_eq!(mesh.triangles[0].indices, [0, 1, 2]);
    assert_eq!(mesh.triangles[0].material_id, 4);
  }

  #[test]
  fn decodes_a_length_prefixed_snapshot() {
    let message = valid_message();
    let mut frame = (message.len() as u32).to_le_bytes().to_vec();
    frame.extend_from_slice(&message);

    let mesh = read_mesh_snapshot(&mut std::io::Cursor::new(frame)).unwrap();
    assert_eq!(mesh.vertices, vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    assert_eq!(mesh.dirty_aabb, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
  }

  #[test]
  fn rejects_bad_magic_and_non_finite_vertex_data() {
    let mut bad_magic = valid_message();
    bad_magic[0] = b'X';
    assert!(parse_mesh_snapshot(&bad_magic).unwrap_err().to_string().contains("magic"));

    let mut non_finite = valid_message();
    non_finite[HEADER_SIZE..HEADER_SIZE + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(parse_mesh_snapshot(&non_finite).unwrap_err().to_string().contains("non-finite"));
  }

  #[test]
  fn rejects_incorrect_declared_payload_length() {
    let mut message = valid_message();
    message.pop();
    assert!(parse_mesh_snapshot(&message).unwrap_err().to_string().contains("expected"));
  }

  #[test]
  fn rejects_an_out_of_range_triangle_index() {
    let mut message = valid_message();
    let index_offset = HEADER_SIZE + 3 * 3 * size_of::<f32>();
    message[index_offset..index_offset + 4].copy_from_slice(&3_u32.to_le_bytes());
    assert!(parse_mesh_snapshot(&message).is_err());
  }
}
