//! Candidates: changes to the bot that are being screened. Each sits behind a switch that is OFF by default, so one
//! binary holds many and one batch runs them against the same base rows (the development repository's candidates queue).
//!
//!   if crate::cand::on("NAME") && crate::ckpt::armed(self.ticks) { ... }
//!
//! On when the name is in MCSIM_CAND (comma-separated: `MCSIM_CAND=a,b`), or in `FORCED` below - which
//! a remote runner's script set in a copy of the tree, since that runner passed no environment. A candidate that is taken becomes
//! a revision in rev.rs (on by default, MCB_NO_<NAME>) with its Java hunk; one that is not is deleted.
use std::sync::OnceLock;

/// (set in a tree copy for a runner that passes no environment; empty in the repository)
pub const FORCED: &str = "";

fn list() -> &'static Vec<String> {
    static V: OnceLock<Vec<String>> = OnceLock::new();
    V.get_or_init(|| {
        let env = std::env::var("MCSIM_CAND").unwrap_or_default();
        FORCED.split(',').chain(env.split(',')).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    })
}

/// Is candidate `name` switched on in this run?
pub fn on(name: &str) -> bool { list().iter().any(|n| n == name) }

/// A numeric setting of a candidate: MCSIM_<NAME> if set and a number, else the default.
pub fn num(var: &str, default: i64) -> i64 { std::env::var(var).ok().and_then(|v| v.parse().ok()).unwrap_or(default) }
