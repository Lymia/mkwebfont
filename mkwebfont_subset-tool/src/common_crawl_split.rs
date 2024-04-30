use crate::common_crawl_download::COMMON_CRAWL_TAG;
use anyhow::Result;
use mkwebfont_common::{
    join_set::JoinSet,
    model::{
        bitset_list::BitsetList,
        data_package::{DataPackage, DataPackageEncoder},
    },
};
use tracing::info;

pub const SPLIT_SECTION_COUNT: usize = 100;
pub const SPLIT_SECTION_DIR: &str = "run/common-crawl_bitsets-training_split";
pub const SPLIT_SECTION_TAG: &str = "common_crawl-split_bitsets";
const SPLIT_VERSION: &str = "v0.1.0";

pub async fn split_common_crawl() -> Result<()> {
    let bitsets = {
        let mut package = DataPackage::load("run/common-crawl_bitsets-training")?;

        BitsetList::deserialize(package.take_section(COMMON_CRAWL_TAG)?)?
    };
    info!("Done!");

    std::fs::create_dir_all(SPLIT_SECTION_DIR)?;
    let mut joins = JoinSet::new();
    for (i, list) in bitsets.split(SPLIT_SECTION_COUNT).into_iter().enumerate() {
        joins.spawn(async move {
            let name = format!("{SPLIT_SECTION_TAG}/{i}/{SPLIT_VERSION}");
            let mut encoder = DataPackageEncoder::new(&name);
            encoder.insert_section(SPLIT_SECTION_TAG, list.serialize(&name)?);
            encoder
                .build()
                .save(format!("{SPLIT_SECTION_DIR}/section_{i}"))?;
            Ok(())
        });
    }
    joins.join().await?;

    Ok(())
}
