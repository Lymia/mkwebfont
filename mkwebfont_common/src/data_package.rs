use anyhow::{bail, ensure, Result};
use bincode::{config, Decode, Encode};
use std::{
    collections::HashMap,
    io::{Cursor, Write},
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::{debug, info};
use xxhash_rust::xxh3::Xxh3Builder;
use zstd::Encoder;

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

    pub fn build(self) -> DataPackage {
        self.0
    }
}

#[derive(Encode, Decode)]
pub struct DataPackage {
    package_id: String,
    timestamp: u64,
    meta_num: HashMap<String, i64, Xxh3Builder>,
    files: HashMap<String, Vec<u8>, Xxh3Builder>,
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

    pub fn encode(&self) -> Result<Vec<u8>> {
        info!("Encoding data package...");
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

    pub fn deserialize(data: &[u8]) -> Result<Self> {
        ensure!(data.len() > 76);
        ensure!(&data[0..8] == MAGIC);
        ensure!(&data[8..12] == VERSION_TAG);

        let data_hash = &data[12..44];
        let compressed_hash = &data[44..76];
        let compressed = &data[76..];

        ensure!(blake3::hash(compressed).as_bytes().as_slice() == compressed_hash);
        let data = zstd::decode_all(Cursor::new(compressed))?;
        ensure!(blake3::hash(&data).as_bytes().as_slice() == data_hash);

        Ok(bincode::decode_from_slice(&data, config::standard())?.0)
    }
}
