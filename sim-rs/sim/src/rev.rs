//! Which commit of the Java bot is being predicted.
//!
//! The port follows the newest Java commit it has been brought to (`port/java/COMMIT`); earlier behaviour that
//! still has reference results stays reachable (`--rev` or `MCSIM_REV`). A change that came with a Java switch
//! (`MCB_NO_...`) is guarded by that switch here, under the same environment variable; one that came without is
//! guarded by the revision (`a1`).
//!
//! From `mend` on the simulator leads: a change is made and judged here first and the Java bot follows (ported and
//! checked by another party), so those revisions are named after what they change, not after a Java commit.
//!
//! Adding a revision: a new flag (`a2` ...) that is true from that commit on, a new arm in `make` (with the flags
//! of all earlier revisions on), its name in `NAMES`, and the "latest" alias moved to it. Dropping one: delete
//! its arm, then follow the compiler / grep for its flags and keep the newer branch.
use std::sync::OnceLock;

pub struct Rev {
    pub name: &'static str,
    /// at or after 2d9a193 (a's master of 2026-11-13: remembered ways out, hazards, last-resort placements, end game)
    pub a1: bool,
    /// at or after `mend` (the first change made in the simulator before the Java bot had it): after the last
    /// clean-up turns, one more pass over the cells the bot broke out itself - nothing new put up, no breaking out
    pub mend: bool,
    /// at or after `orphan`: a cell the main pass is about to put off gets one try with the helper on a second helper
    pub orphan: bool,
    /// at or after `close`: a helper block on every side of the cell in turn, the fine search of place_close with the
    /// helper standing, and that search over a wider range of places
    pub close: bool,
    // ---- the Java version's switches (2d9a193), all on unless the variable is set
    /// `BuildState.X_EXIT` (MCB_NO_EXIT): the trap check searches for a real way to the chests and remembers it
    pub x_exit: bool,
    /// `X_REL` (MCB_NO_REL): a block that wedges the bot in is refused; one that shuts in a bot already shut in is not
    pub x_rel: bool,
    /// `X_UNSTICK` (MCB_NO_UNSTICK): takes a thin thing out of the bot's own cell
    pub x_unstick: bool,
    /// `X_LOOP` (MCB_NO_LOOP): the clean-up comes back to what it set aside
    pub x_loop: bool,
    /// `X_KEEP` (MCB_NO_KEEP): a temporary block that is the only way out stays for now
    pub x_keep: bool,
    /// `X_NEXT` (MCB_NO_NEXT): temporary blocks in a column's next cell, for ways out and routes not to be had otherwise
    pub x_next: bool,
    /// `Grid.X_HAZ` (MCB_NO_HAZ): never stand on or in what hurts
    pub x_haz: bool,
    /// `Grid.X_CARPET` (MCB_NO_CARPET): a carpet on a fence stands as high as the fence
    pub x_carpet: bool,
    /// `Nav.X_FPILLAR` (MCB_NO_FPILLAR): pillar up from the top of a fence or wall
    pub x_fpillar: bool,
    /// `Builder.MONITOR` (MCB_NO_MONITOR): the sampled "is there a way back to the chests" check (statistics only)
    pub monitor: bool,
    /// `Scaffolds.CAREFUL` (MCB_CAREFUL, off unless set): clean-up only from where the bot can walk away afterwards
    pub careful: bool,
    /// `MCB_NO_HELPER` not set: the trap check takes the helper block away in thought
    pub helper: bool,
}

static REV: OnceLock<Rev> = OnceLock::new();

pub const NAMES: [&str; 5] = ["f6b21f4", "2d9a193", "mend", "orphan", "close"];

fn make(name: &str) -> Rev {
    let on = |v: &str| std::env::var_os(v).is_none();
    match name {
        "f6b21f4" | "base" => Rev { name: "f6b21f4", a1: false, mend: false, orphan: false, close: false, x_exit: false, x_rel: false, x_unstick: false, x_loop: false, x_keep: false, x_next: false, x_haz: false, x_carpet: false, x_fpillar: false, monitor: false, careful: false, helper: false },
        "2d9a193" | "a1" | "mend" | "orphan" | "close" | "latest" => Rev {
            name: match name { "2d9a193" | "a1" => "2d9a193", "mend" => "mend", "orphan" => "orphan", _ => "close" }, a1: true, mend: !(name == "2d9a193" || name == "a1"),
            orphan: !(name == "2d9a193" || name == "a1" || name == "mend"), close: !(name == "2d9a193" || name == "a1" || name == "mend" || name == "orphan"), x_exit: on("MCB_NO_EXIT"), x_rel: on("MCB_NO_REL"), x_unstick: on("MCB_NO_UNSTICK"), x_loop: on("MCB_NO_LOOP"), x_keep: on("MCB_NO_KEEP"), x_next: on("MCB_NO_NEXT"),
            x_haz: on("MCB_NO_HAZ"), x_carpet: on("MCB_NO_CARPET"), x_fpillar: on("MCB_NO_FPILLAR"), monitor: on("MCB_NO_MONITOR"), careful: !on("MCB_CAREFUL"), helper: on("MCB_NO_HELPER"),
        },
        o => { eprintln!("unknown revision {o} (known: {})", NAMES.join(", ")); std::process::exit(2) }
    }
}

/// Chooses the revision (once, before anything asks for it).
pub fn set(name: &str) { if REV.set(make(name)).is_err() { panic!("revision already chosen"); } }

#[inline]
pub fn rev() -> &'static Rev {
    REV.get_or_init(|| make(&std::env::var("MCSIM_REV").unwrap_or_else(|_| "latest".to_string())))
}
