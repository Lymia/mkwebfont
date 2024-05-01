use crate::{
    generate_adjacency_table::{ADJACENCY_PATH, ADJACENCY_TAG},
    generate_gfsubsets::{GFSUBSETS_PATH, GFSUBSETS_TAG},
    generate_glyphsets::{GLYPHSETS_PATH, GLYPHSETS_TAG},
    generate_validation_data::{VALIDATION_DATA_PATH, VALIDATION_DATA_TAG},
};
use anyhow::Result;
use mkwebfont_common::{
    join_set::JoinSet,
    model::{
        data_package::{DataPackage, DataPackageEncoder},
        package_consts::{
            ADJACENCY_PACKAGE_NAME, BUILTIN_PACKAGE_NAME, PKG_ADJACENCY_TAG, PKG_GFSUBSETS_TAG,
            PKG_GLYPHSETS_TAG, PKG_VALIDATION_TAG, VALID_PACKAGE_NAME,
        },
    },
};

pub async fn generate_data() -> Result<()> {
    let mut adjacency_array_pkg = DataPackage::load(ADJACENCY_PATH)?;
    let adjacency_array = adjacency_array_pkg.take_section(ADJACENCY_TAG)?;

    let mut gfsubsets_pkg = DataPackage::load(GFSUBSETS_PATH)?;
    let gfsubsets = gfsubsets_pkg.take_section(GFSUBSETS_TAG)?;

    let mut glyphsets_pkg = DataPackage::load(GLYPHSETS_PATH)?;
    let glyphsets = glyphsets_pkg.take_section(GLYPHSETS_TAG)?;

    let mut validation_data_pkg = DataPackage::load(VALIDATION_DATA_PATH)?;
    let validation_data = validation_data_pkg.take_section(VALIDATION_DATA_TAG)?;

    let mut joins = JoinSet::new();
    joins.spawn(async move {
        let mut pkg = DataPackageEncoder::new(VALID_PACKAGE_NAME);
        pkg.insert_section(PKG_VALIDATION_TAG, validation_data);
        pkg.build().save(&format!("run/{VALID_PACKAGE_NAME}"))?;
        Ok(())
    });
    joins.spawn(async move {
        let mut pkg = DataPackageEncoder::new(ADJACENCY_PACKAGE_NAME);
        pkg.insert_section(PKG_ADJACENCY_TAG, adjacency_array);
        pkg.build().save(&format!("run/{ADJACENCY_PACKAGE_NAME}"))?;
        Ok(())
    });
    joins.spawn(async move {
        let mut pkg = DataPackageEncoder::new(BUILTIN_PACKAGE_NAME);
        pkg.insert_section(PKG_GFSUBSETS_TAG, gfsubsets);
        pkg.insert_section(PKG_GLYPHSETS_TAG, glyphsets);
        pkg.build().save(&format!("run/{BUILTIN_PACKAGE_NAME}"))?;
        Ok(())
    });
    joins.join().await?;

    Ok(())
}
