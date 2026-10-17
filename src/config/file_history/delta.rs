use sha2::{Digest, Sha256};
use std::io::{self, Read};

const MAGIC: &[u8; 4] = b"VHD1";
const HEADER_BYTES: usize = 4 + 16 + 4 + 4 + 32 + 32;

fn invalid_delta() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "历史增量损坏或基准不匹配")
}

pub(super) struct Delta<'a> {
    pub(super) base_sequence: u128,
    prefix: usize,
    suffix: usize,
    base_hash: [u8; 32],
    result_hash: [u8; 32],
    inserted: &'a [u8],
}

impl<'a> Delta<'a> {
    pub(super) fn decode(mut bytes: &'a [u8]) -> io::Result<Self> {
        let mut magic = [0; 4];
        let mut sequence = [0; 16];
        let mut prefix = [0; 4];
        let mut suffix = [0; 4];
        let mut base_hash = [0; 32];
        let mut result_hash = [0; 32];
        bytes.read_exact(&mut magic)?;
        bytes.read_exact(&mut sequence)?;
        bytes.read_exact(&mut prefix)?;
        bytes.read_exact(&mut suffix)?;
        bytes.read_exact(&mut base_hash)?;
        bytes.read_exact(&mut result_hash)?;
        if &magic != MAGIC {
            return Err(invalid_delta());
        }
        Ok(Self {
            base_sequence: u128::from_le_bytes(sequence),
            prefix: u32::from_le_bytes(prefix) as usize,
            suffix: u32::from_le_bytes(suffix) as usize,
            base_hash,
            result_hash,
            inserted: bytes,
        })
    }

    pub(super) fn apply(&self, base: &[u8], maximum: usize) -> io::Result<Vec<u8>> {
        let base_hash: [u8; 32] = Sha256::digest(base).into();
        if base_hash != self.base_hash {
            return Err(invalid_delta());
        }
        let suffix_start = base
            .len()
            .checked_sub(self.suffix)
            .ok_or_else(invalid_delta)?;
        if self.prefix > suffix_start {
            return Err(invalid_delta());
        }
        let size = self
            .prefix
            .checked_add(self.inserted.len())
            .and_then(|size| size.checked_add(self.suffix))
            .filter(|size| *size <= maximum)
            .ok_or_else(invalid_delta)?;
        let mut result = Vec::with_capacity(size);
        result.extend_from_slice(base.get(..self.prefix).ok_or_else(invalid_delta)?);
        result.extend_from_slice(self.inserted);
        result.extend_from_slice(base.get(suffix_start..).ok_or_else(invalid_delta)?);
        let result_hash: [u8; 32] = Sha256::digest(&result).into();
        if result_hash != self.result_hash {
            return Err(invalid_delta());
        }
        Ok(result)
    }
}

pub(super) fn encode(base_sequence: u128, before: &[u8], after: &[u8]) -> Option<Vec<u8>> {
    let prefix = before
        .iter()
        .zip(after)
        .take_while(|(left, right)| left == right)
        .count();
    let suffix = before
        .iter()
        .skip(prefix)
        .rev()
        .zip(after.iter().skip(prefix).rev())
        .take_while(|(left, right)| left == right)
        .count();
    let inserted = after.get(prefix..after.len().checked_sub(suffix)?)?;
    if HEADER_BYTES.checked_add(inserted.len())? >= after.len() {
        return None;
    }
    let mut payload = Vec::with_capacity(HEADER_BYTES + inserted.len());
    payload.extend_from_slice(MAGIC);
    payload.extend_from_slice(&base_sequence.to_le_bytes());
    payload.extend_from_slice(&u32::try_from(prefix).ok()?.to_le_bytes());
    payload.extend_from_slice(&u32::try_from(suffix).ok()?.to_le_bytes());
    payload.extend_from_slice(&Sha256::digest(before));
    payload.extend_from_slice(&Sha256::digest(after));
    payload.extend_from_slice(inserted);
    Some(payload)
}
