use anyhow::Result;
use std::io::Cursor;

pub fn zstd_compress(data: &[u8]) -> Result<Vec<u8>> {
    Ok(zstd::encode_all(Cursor::new(data), 10)?)
}

pub fn zstd_decompress(data: &[u8]) -> Result<Vec<u8>> {
    Ok(zstd::decode_all(Cursor::new(data))?)
}
