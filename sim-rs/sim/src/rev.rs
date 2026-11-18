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
    /// at or after `chain`: the helper on a second helper also in builds with pistons, after the main pass
    pub chain: bool,
    /// at or after `careful`: the careful clean-up is on (unless MCB_NO_CAREFUL), and its question "is there a
    /// plain way to the chests" joins and feeds the remembered way out
    pub careful_join: bool,
    /// at or after `fewfloat`: in the careful clean-up the stance planner tries at most two stands hung in the air,
    /// each with a small search (MCB_FLOAT_CAREFUL=1: as before)
    pub few_float: bool,
    /// at or after `escape` (unless MCB_NO_ESCAPE): a bot that is enclosed - the search that may break through has
    /// looked at every place it can get to, 64 at most, and none is the way - may on its last try break a block
    /// unless a neighbour would really come off with it (before: unless any neighbour is of a kind that needs support)
    pub escape: bool,
    /// at or after `askonce` (unless MCB_NO_PFHEUR): the searches of `way_out` that are not settled after 1000 cells go
    /// on looking at every place once (`Bot::nav_ask`)
    pub pf_heur: bool,
    /// at or after `askonce` (unless MCB_NO_PFTUN): `tunnel_to` first asks once for a way to any of the places next
    /// to the cell (`Bot::tunnel_hopeless`)
    pub pf_tun: bool,
    /// at or after `plankeep` (unless MCB_NO_PLANKEEP): in one stance plan the route search for the first column or
    /// hung block is kept, and the later candidates are first put to it, continued; only one it reaches gets the
    /// search from zero (which decides, as before)
    pub plan_keep: bool,
    /// at or after `thoughtway` (unless MCB_NO_THOUGHTWAY): the clean-up's "can I still get away with these pieces
    /// gone" keeps the way it finds for as long as the same world-in-thought is asked about, and later candidate
    /// stands only have to reach a place on it (`Bot::safe_after_removal`)
    pub thought_way: bool,
    /// MCB_OWNDOWN (a measurement, off: not taken on 2026-11-18 - within noise over the seeds): in the clean-up, what
    /// the tries at one place put up to get at their pieces is taken down when the next piece lies somewhere else,
    /// while the bot is still there (`Bot::own_down_now`)
    pub own_down: bool,
    /// at or after `repair` (unless MCB_NO_REPAIR): when the programme is over and sections still fail by the bot's own
    /// picture, it takes them up again - up to three rounds of main loop, helper and trap rules and the end game
    /// (`Bot::repair`, bot/repair.rs)
    pub repair: bool,
    /// at or after `ropefar` (unless MCB_NO_ROPEFAR): while it builds, the bot does not walk back for pieces of its own
    /// way (pillar, bridge) that lie 6 or more blocks behind it; they stand as ordinary scaffolding for the clean-up
    /// (`Bot::undo_rope_impl`)
    pub rope_far: bool,
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

pub const NAMES: [&str; 14] = ["f6b21f4", "2d9a193", "mend", "orphan", "close", "chain", "careful", "fewfloat", "escape", "askonce", "plankeep", "thoughtway", "repair", "ropefar"];

fn make(name: &str) -> Rev {
    let on = |v: &str| std::env::var_os(v).is_none();
    match name {
        "f6b21f4" | "base" => Rev { name: "f6b21f4", a1: false, mend: false, orphan: false, close: false, chain: false, careful_join: false, few_float: false, escape: false, pf_heur: false, pf_tun: false, plan_keep: false, thought_way: false, own_down: false, repair: false, rope_far: false, x_exit: false, x_rel: false, x_unstick: false, x_loop: false, x_keep: false, x_next: false, x_haz: false, x_carpet: false, x_fpillar: false, monitor: false, careful: false, helper: false },
        "2d9a193" | "a1" | "mend" | "orphan" | "close" | "chain" | "careful" | "fewfloat" | "escape" | "askonce" | "plankeep" | "thoughtway" | "repair" | "ropefar" | "latest" => Rev {
            name: match name { "2d9a193" | "a1" => "2d9a193", "mend" => "mend", "orphan" => "orphan", "close" => "close", "chain" => "chain", "careful" => "careful", "fewfloat" => "fewfloat", "escape" => "escape", "askonce" => "askonce", "plankeep" => "plankeep", "thoughtway" => "thoughtway", "repair" => "repair", _ => "ropefar" }, a1: true, mend: !(name == "2d9a193" || name == "a1"),
            chain: !matches!(name, "2d9a193" | "a1" | "mend" | "orphan" | "close"), careful_join: !matches!(name, "2d9a193" | "a1" | "mend" | "orphan" | "close" | "chain"), few_float: !matches!(name, "2d9a193" | "a1" | "mend" | "orphan" | "close" | "chain" | "careful"), escape: matches!(name, "escape" | "askonce" | "plankeep" | "thoughtway" | "repair" | "ropefar" | "latest") && on("MCB_NO_ESCAPE") && on("MCB_FLOAT_CAREFUL"),
            pf_heur: matches!(name, "askonce" | "plankeep" | "thoughtway" | "repair" | "ropefar" | "latest") && on("MCB_NO_PFHEUR"), pf_tun: matches!(name, "askonce" | "plankeep" | "thoughtway" | "repair" | "ropefar" | "latest") && on("MCB_NO_PFTUN"), plan_keep: matches!(name, "plankeep" | "thoughtway" | "repair" | "ropefar" | "latest") && on("MCB_NO_PLANKEEP"), thought_way: matches!(name, "thoughtway" | "repair" | "ropefar" | "latest") && on("MCB_NO_THOUGHTWAY"), own_down: std::env::var_os("MCB_OWNDOWN").is_some(), repair: matches!(name, "repair" | "ropefar" | "latest") && on("MCB_NO_REPAIR"), rope_far: matches!(name, "ropefar" | "latest") && on("MCB_NO_ROPEFAR"),
            orphan: !(name == "2d9a193" || name == "a1" || name == "mend"), close: !(name == "2d9a193" || name == "a1" || name == "mend" || name == "orphan"), x_exit: on("MCB_NO_EXIT"), x_rel: on("MCB_NO_REL"), x_unstick: on("MCB_NO_UNSTICK"), x_loop: on("MCB_NO_LOOP"), x_keep: on("MCB_NO_KEEP"), x_next: on("MCB_NO_NEXT"),
            x_haz: on("MCB_NO_HAZ"), x_carpet: on("MCB_NO_CARPET"), x_fpillar: on("MCB_NO_FPILLAR"), monitor: on("MCB_NO_MONITOR"), careful: if matches!(name, "2d9a193" | "a1" | "mend" | "orphan" | "close" | "chain") { !on("MCB_CAREFUL") } else { on("MCB_NO_CAREFUL") }, helper: on("MCB_NO_HELPER"),
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
