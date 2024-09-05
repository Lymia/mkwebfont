pub mod character_set;
pub mod compression;
pub mod download_cache;
pub mod hashing;
pub mod join_set;
pub mod paths;

pub const FILTER_SPEC: &str =
    "debug,h2=info,hyper_util=info,reqwest=info,rustls=info,selectors=info,html5ever=info,ureq=info";
