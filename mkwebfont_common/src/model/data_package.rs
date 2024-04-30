use crate::wyhash::WyHashBuilder;
use anyhow::{bail, ensure, Result};
use bincode::{config, Decode, Encode};
use blake3::Hasher;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, Cursor, Read, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::{debug, info};
use zstd::{Decoder, Encoder};

const MAGIC: &[u8; 8] = b"mkwbfont";
const VERSION_TAG: &[u8; 4] = b"v0.1";

pub struct DataPackageEncoder(DataPackage);
impl DataPackageEncoder {
    pub fn new(id: &str) -> Self {
        let unix_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        DataPackageEncoder(DataPackage {
            package_id: id.to_string(),
            timestamp: unix_time.as_secs(),
            meta_num: Default::default(),
            files: Default::default(),
        })
    }

    pub fn insert_i64(&mut self, key: &str, data: i64) {
        assert!(!self.0.meta_num.contains_key(key), "Duplicate meta number {key}!");
        self.0.meta_num.insert(key.to_string(), data);
    }

    pub fn insert_u64(&mut self, key: &str, data: u64) {
        self.insert_i64(key, data as i64);
    }

    pub fn insert_data(&mut self, key: &str, data: Vec<u8>) {
        assert!(!self.0.files.contains_key(key), "Duplicate data package section {key}!");
        self.0.files.insert(key.to_string(), data);
    }

    pub fn insert_bincode<T: Encode>(&mut self, key: &str, data: &T) {
        self.insert_data(key, bincode::encode_to_vec(data, config::standard()).unwrap());
    }

    pub fn build(self) -> DataPackage {
        self.0
    }
}

#[derive(Encode, Decode)]
pub struct DataPackage {
    package_id: String,
    timestamp: u64,
    meta_num: HashMap<String, i64, WyHashBuilder>,
    files: HashMap<String, Vec<u8>, WyHashBuilder>,
}
impl DataPackage {
    pub fn package_id(&self) -> &str {
        &self.package_id
    }

    pub fn get_i64(&self, key: &str) -> Result<i64> {
        if let Some(x) = self.meta_num.get(key) {
            Ok(*x)
        } else {
            bail!("No data package section {key}");
        }
    }

    pub fn get_u64(&self, key: &str) -> Result<u64> {
        self.get_i64(key).map(|x| x as u64)
    }

    pub fn get_data(&self, key: &str) -> Result<&[u8]> {
        if let Some(x) = self.files.get(key) {
            Ok(x.as_slice())
        } else {
            bail!("No data package section {key}");
        }
    }

    pub fn take_data(&mut self, key: &str) -> Result<Vec<u8>> {
        if let Some(x) = self.files.remove(key) {
            Ok(x)
        } else {
            bail!("No data package section {key}");
        }
    }

    pub fn get_bincode<T: Decode>(&self, key: &str) -> Result<T> {
        let bytes = self.get_data(key)?;
        let (value, count) = bincode::decode_from_slice(bytes, config::standard())?;
        assert_eq!(count, bytes.len());
        Ok(value)
    }

    pub fn take_bincode<T: Decode>(&mut self, key: &str) -> Result<T> {
        let bytes = self.take_data(key)?;
        let (value, count) = bincode::decode_from_slice(&bytes, config::standard())?;
        assert_eq!(count, bytes.len());
        Ok(value)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        info!("Encoding data package: {}...", self.package_id);
        let data = bincode::encode_to_vec(self, config::standard())?;

        debug!("Compressing data package...");
        let compressed = {
            let cursor = Cursor::new(Vec::<u8>::new());
            let mut zstd = Encoder::new(cursor, 20)?;
            zstd.multithread(8)?;
            zstd.set_pledged_src_size(Some(data.len() as u64))?;
            zstd.write_all(&data)?;
            zstd.finish()?.into_inner()
        };

        debug!("Building data package...");
        let data_hash = blake3::hash(&data);
        let compressed_hash = blake3::hash(&compressed);

        let mut encoded = Vec::new();
        encoded.extend(MAGIC.as_slice());
        encoded.extend(VERSION_TAG.as_slice());
        encoded.extend(data_hash.as_bytes().as_slice());
        encoded.extend(compressed_hash.as_bytes().as_slice());
        encoded.extend(compressed);

        Ok(encoded)
    }

    pub fn save(&self, target: impl AsRef<Path>) -> Result<()> {
        std::fs::write(target, self.encode()?)?;
        Ok(())
    }

    fn deserialize_stream(mut r: impl Read) -> Result<Self> {
        let mut header = [0u8; 76];
        r.read_exact(&mut header)?;

        ensure!(&header[0..8] == MAGIC);
        ensure!(&header[8..12] == VERSION_TAG);

        let data_hash = &header[12..44];
        let compressed_hash = &header[44..76];

        // construct the reader chain
        let blake3_r0 = Blake3Reader::new(r);
        let mut buf_r1 = BufReader::new(blake3_r0);
        let zstd = Decoder::with_buffer(&mut buf_r1)?;
        let blake3_r2 = Blake3Reader::new(zstd);
        let mut buf_r3 = BufReader::new(blake3_r2);

        let val: Self = bincode::decode_from_reader(&mut buf_r3, config::standard())?;

        let blake3_r2 = buf_r3.into_inner();
        assert_eq!(blake3_r2.hash.finalize().as_bytes(), data_hash);
        drop(blake3_r2);
        assert_eq!(buf_r1.into_inner().hash.finalize().as_bytes(), compressed_hash);

        Ok(val)
    }

    pub fn deserialize(data: &[u8]) -> Result<Self> {
        Self::deserialize_stream(Cursor::new(data))
    }

    pub fn load(target: impl AsRef<Path>) -> Result<Self> {
        Self::deserialize_stream(File::open(target)?)
    }
}

struct Blake3Reader<R: Read> {
    hash: Hasher,
    underlying: R,
}
impl<R: Read> Blake3Reader<R> {
    pub fn new(underlying: R) -> Self {
        Blake3Reader { hash: Hasher::new(), underlying }
    }
}
impl<R: Read> Read for Blake3Reader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let result = self.underlying.read(buf)?;
        self.hash.write_all(&buf[..result])?;
        Ok(result)
    }
}
