use anyhow::Result;
use mkwebfont_common::{
    join_set::JoinSet,
    model::{
        bitset_list::BitsetList,
        data_package::{DataPackage, DataPackageEncoder},
    },
};
use tracing::info;

pub const SECTION_COUNT: usize = 100;
pub const SECTION_DIR: &str = "run/common-crawl_bitsets-training_split";
pub const SECTION_TABLE: &str = "bitset_list";

pub async fn split_common_crawl() -> Result<()> {
    let bitsets = {
        info!("Loading raw bitsets...");
        let package = DataPackage::load("run/common-crawl_bitsets-training")?;
        BitsetList::deserialize("bitset_list", &package)?
    };
    info!("Done!");

    std::fs::create_dir_all(SECTION_DIR)?;
    let mut joins = JoinSet::new();
    for (i, list) in bitsets.split(SECTION_COUNT).into_iter().enumerate() {
        joins.spawn(async move {
            let mut encoder = DataPackageEncoder::new(&format!("raw_adjacency_{i}"));
            list.serialize(SECTION_TABLE, &mut encoder)?;
            encoder.build().save(format!("{SECTION_DIR}/section_{i}"))?;
            Ok(())
        });
    }
    joins.join().await?;

    Ok(())
}
