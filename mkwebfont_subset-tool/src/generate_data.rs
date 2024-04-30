use crate::{
    generate_adjacency_table::{RAW_ADJACENCY_PATH, RAW_ADJACENCY_TAG},
    generate_gfsubsets::{GFSUBSETS_PATH, GFSUBSETS_TAG},
};
use anyhow::Result;
use mkwebfont_common::model::{
    adjacency_array::AdjacencyArray,
    data_package::{DataPackage, DataPackageEncoder},
    package_consts::{PACKAGE_NAME, PKG_ADJACENCY_TAG, PKG_GFSUBSETS_TAG},
    subset_data::RawSubset,
};

pub fn generate_data() -> Result<()> {
    let mut adjacency_array_pkg = DataPackage::load(RAW_ADJACENCY_PATH)?;
    let adjacency_array = AdjacencyArray::deserialize(RAW_ADJACENCY_TAG, &mut adjacency_array_pkg)?;

    let mut gfsubsets_pkg = DataPackage::load(GFSUBSETS_PATH)?;
    let gfsubsets: Vec<RawSubset> = gfsubsets_pkg.take_bincode(GFSUBSETS_TAG)?;

    let mut pkg = DataPackageEncoder::new(PACKAGE_NAME);
    adjacency_array.serialize(PKG_ADJACENCY_TAG, &mut pkg)?;
    pkg.insert_bincode(PKG_GFSUBSETS_TAG, &gfsubsets);
    pkg.build().save(&format!("run/{PACKAGE_NAME}"))?;

    Ok(())
}
