use crate::{
    generate_adjacency_table::{ADJACENCY_PATH, ADJACENCY_TAG},
    generate_gfsubsets::{GFSUBSETS_PATH, GFSUBSETS_TAG},
};
use anyhow::Result;
use mkwebfont_common::model::{
    data_package::{DataPackage, DataPackageEncoder},
    package_consts::{PACKAGE_NAME, PKG_ADJACENCY_TAG, PKG_GFSUBSETS_TAG},
};

pub fn generate_data() -> Result<()> {
    let mut adjacency_array_pkg = DataPackage::load(ADJACENCY_PATH)?;
    let adjacency_array = adjacency_array_pkg.take_section(ADJACENCY_TAG)?;

    let mut gfsubsets_pkg = DataPackage::load(GFSUBSETS_PATH)?;
    let gfsubsets = gfsubsets_pkg.take_section(GFSUBSETS_TAG)?;

    let mut pkg = DataPackageEncoder::new(PACKAGE_NAME);
    pkg.insert_section(PKG_ADJACENCY_TAG, adjacency_array);
    pkg.insert_section(PKG_GFSUBSETS_TAG, gfsubsets);
    pkg.build().save(&format!("run/{PACKAGE_NAME}"))?;

    Ok(())
}
