//! Where the simulator finds what is not in git. Every path can be set from the environment; the defaults are
//! inside the repository checkout this crate was built from (the crate sits two levels below its root).
//!
//! * `MCB_WORK`     (default `<repo>/work`): the working directory shared with the Java harness
//! * `MCSIM_REF`    (default `$MCB_WORK/reference`): `mc/states.tsv` and `java-extra/states-extra.tsv`, written by
//!   `harness/run.sh tables`
//! * `MCSIM_DATA`   (default `<repo>/sim-rs/data`): the block, item and tag tables (in git)
//! * `MCSIM_CORPUS` (default `$MCB_WORK/corpus`): normalised schematics, `cache/<2 hex>/<hash>.mcb`, written by
//!   `harness/run.sh prep`
fn var_or(name: &str, default: impl FnOnce() -> String) -> String {
    std::env::var(name).ok().filter(|v| !v.is_empty()).unwrap_or_else(default)
}
pub fn repo() -> String { format!("{}/../..", env!("CARGO_MANIFEST_DIR")) }
pub fn work() -> String { var_or("MCB_WORK", || format!("{}/work", repo())) }
pub fn reference() -> String { var_or("MCSIM_REF", || format!("{}/reference", work())) }
pub fn data() -> String { var_or("MCSIM_DATA", || format!("{}/sim-rs/data", repo())) }
pub fn corpus() -> String { var_or("MCSIM_CORPUS", || format!("{}/corpus", work())) }
pub fn tmp() -> String { format!("{}/tmp", work()) }
