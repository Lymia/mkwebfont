use anyhow::*;
use std::path::Path;

pub fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub fn get_relative_fragment(parent: &Path, child: &Path) -> Result<String> {
    let parent = parent.canonicalize()?;
    let child = child.canonicalize()?;

    let parent = path_to_string(&parent);
    let child = path_to_string(&child);

    if child.starts_with(&parent) {
        Ok(child[parent.len()..].to_string())
    } else {
        bail!("'{parent}' is not a parent of '{child}'");
    }
}

pub fn get_relative_from(root: &Path, target: &Path) -> Result<String> {
    let root = root.canonicalize()?;
    let target = target.canonicalize()?;

    let mut accum = root.as_path();
    let mut super_frag = String::new();
    loop {
        if let Some(new_accum) = accum.parent() {
            accum = new_accum;
        } else {
            bail!("{} and {} share no common parent!", root.display(), target.display());
        }

        if target.starts_with(accum) {
            let fragment = get_relative_fragment(accum, &target)?;
            return Ok(format!("{super_frag}{fragment}"));
        }

        super_frag.push_str("../");
    }
}
