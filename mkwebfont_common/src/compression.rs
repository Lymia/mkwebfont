use anyhow::Result;
use brotli::{CompressorReader, Decompressor};
use std::io::{Cursor, Read};

pub fn brotli_compress(data: &[u8]) -> Result<Vec<u8>> {
    let mut reader = CompressorReader::with_params(Cursor::new(data), 0, &Default::default());
    let mut out = Vec::new();
    reader.read_to_end(&mut out)?;
    Ok(out)
}

pub fn brotli_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut reader = Decompressor::new(Cursor::new(data), 0);
    let mut out = Vec::new();
    reader.read_to_end(&mut out)?;
    Ok(out)
}
