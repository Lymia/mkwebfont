use anyhow::{bail, ensure, Result};
use bincode::{config, Decode, Encode};
use blake3::Hasher;
use std::{
    collections::BTreeMap,
    fmt::{Debug, Formatter},
    fs::File,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::{debug, info};
use zstd::{Decoder, Encoder};

const MAGIC: &[u8; 8] = b"mkwbfont";
const VERSION_TAG: &[u8; 4] = b"v0.2";

pub struct DataSectionEncoder(DataSection);
impl DataSectionEncoder {
    pub fn new(tag: &str, tp: &str) -> Self {
        let unix_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        DataSectionEncoder(DataSection {
            tag: tag.to_string(),
            tp: tp.to_string(),
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

    pub fn build(self) -> DataSection {
        self.0
    }
}

#[derive(Encode, Decode)]
pub struct DataSection {
    tag: String,
    tp: String,
    timestamp: u64,
    meta_num: BTreeMap<String, i64>,
    files: BTreeMap<String, Vec<u8>>,
}
impl DataSection {
    pub fn type_check(&self, tp: &str) -> Result<()> {
        if tp == self.tp {
            bail!("DataSection type mismatch: {tp:?} != {:?}", self.tp);
        } else {
            debug!("Deserializing {tp} from {self:?}...");
            Ok(())
        }
    }

    pub fn get_i64(&self, key: &str) -> Result<i64> {
        if let Some(x) = self.meta_num.get(key) {
            Ok(*x)
        } else {
            bail!("No data section num: {key}");
        }
    }

    pub fn get_u64(&self, key: &str) -> Result<u64> {
        self.get_i64(key).map(|x| x as u64)
    }

    pub fn take_data(&mut self, key: &str) -> Result<Vec<u8>> {
        if let Some(x) = self.files.remove(key) {
            Ok(x)
        } else {
            bail!("No data section file: {key}");
        }
    }

    pub fn take_bincode<T: Decode>(&mut self, key: &str) -> Result<T> {
        let bytes = self.take_data(key)?;
        let (value, count) = bincode::decode_from_slice(&bytes, config::standard())?;
        assert_eq!(count, bytes.len());
        Ok(value)
    }
}
impl Debug for DataSection {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "[DataSection {:?}, timestamp {}]", self.tag, self.timestamp)
    }
}

pub struct DataPackageEncoder(DataPackage);
impl DataPackageEncoder {
    pub fn new(id: &str) -> Self {
        let unix_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        DataPackageEncoder(DataPackage {
            package_id: id.to_string(),
            timestamp: unix_time.as_secs(),
            packages: Default::default(),
        })
    }

    pub fn insert_section(&mut self, key: &str, section: DataSection) {
        self.0.packages.insert(key.to_string(), section);
    }

    pub fn build(self) -> DataPackage {
        self.0
    }
}

#[derive(Encode, Decode)]
pub struct DataPackage {
    package_id: String,
    timestamp: u64,
    packages: BTreeMap<String, DataSection>,
}
impl DataPackage {
    pub fn package_id(&self) -> &str {
        &self.package_id
    }

    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    pub fn take_section(&mut self, key: &str) -> Result<DataSection> {
        if let Some(x) = self.packages.remove(key) {
            Ok(x)
        } else {
            bail!("No data section: {key}");
        }
    }

    pub fn save(&self, target: impl AsRef<Path>) -> Result<()> {
        info!("Serializing data package to '{}'...", target.as_ref().display());

        let mut writer = File::create(target)?;

        // write headers
        writer.write_all(MAGIC)?;
        writer.write_all(VERSION_TAG)?;
        writer.write_all(&[0; 32])?; // temporary

        // compress main body
        let hash = {
            let mut hash = Blake3Writer::new(&mut writer);

            let mut zstd = Encoder::new(&mut hash, 22)?;
            zstd.multithread(16)?;
            bincode::encode_into_std_write(
                self,
                &mut BufWriter::new(&mut zstd),
                config::standard(),
            )?;
            zstd.finish()?;

            *hash.hash.finalize().as_bytes()
        };

        // write the final hash
        writer.seek(SeekFrom::Start((MAGIC.len() + VERSION_TAG.len()) as u64))?;
        writer.write_all(&hash)?;
        Ok(())
    }

    fn deserialize_stream(mut r: impl Read) -> Result<Self> {
        let mut header = [0u8; 44];
        r.read_exact(&mut header)?;

        ensure!(&header[0..8] == MAGIC);
        ensure!(&header[8..12] == VERSION_TAG);
        let compressed_hash = &header[12..44];

        // construct the reader chain
        let blake3_r0 = Blake3Reader::new(r);
        let mut buf_r1 = BufReader::new(blake3_r0);
        let zstd = Decoder::with_buffer(&mut buf_r1)?;
        let blake3_r2 = Blake3Reader::new(zstd);
        let mut buf_r3 = BufReader::new(blake3_r2);

        let val: Self = bincode::decode_from_reader(&mut buf_r3, config::standard())?;

        drop(buf_r3);
        assert_eq!(buf_r1.into_inner().hash.finalize().as_bytes(), compressed_hash);

        Ok(val)
    }

    pub fn load(target: impl AsRef<Path>) -> Result<Self> {
        debug!("Deserializing data package from '{}'...", target.as_ref().display());
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

struct Blake3Writer<W: Write> {
    hash: Hasher,
    underlying: W,
}
impl<W: Write> Blake3Writer<W> {
    pub fn new(underlying: W) -> Self {
        Blake3Writer { hash: Hasher::new(), underlying }
    }
}
impl<W: Write> Write for Blake3Writer<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let result = self.underlying.write(buf)?;
        self.hash.write_all(&buf[..result])?;
        Ok(result)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.underlying.flush()
    }
}
