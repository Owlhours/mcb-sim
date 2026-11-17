//! Block rules: the per-block behaviour of the game that the bot can set off or depends on. `mod.rs` is the
//! dispatch; the rules themselves are ports of the vanilla 26.2 block classes:
//!   place.rs    - what a click with a block item produces (BlockItem.place up to the block write), canSurvive,
//!                 canBeReplaced, support shapes
//!   basic.rs    - updateShape, onPlace, removal, scheduled ticks, right-click behaviour, setPlacedBy ... for
//!                 everything that is not redstone
//!   redstone.rs - signals, wire, torches, repeaters, comparators, pistons, observers, rails, doors/gates
//!                 reacting to power, pressure plates ...
//!   vibrations.rs - game events as sculk sensors hear them (emission sites call `vibrations::emit`)
#![allow(dead_code, unused_variables)]
use crate::geom::*;
use crate::tables::*;
use crate::world::World;

pub mod basic;
pub mod place;
pub mod redstone;
pub mod vibrations;

/// A use-item-on-block click as the placement code sees it (BlockPlaceContext / UseOnContext).
#[derive(Clone, Debug)]
pub struct PlaceCtx {
    /// item in hand (item id; 0 = empty hand)
    pub item: u16,
    /// the block that was clicked, the clicked face and the exact click location (world coordinates)
    pub hit_pos: P,
    pub face: Dir,
    pub hit: [f64; 3],
    /// BlockHitResult.isInside
    pub inside: bool,
    /// view direction of the clicking player (degrees, the game's convention) and whether it is sneaking
    pub yaw: f32,
    pub pitch: f32,
    pub sneak: bool,
    /// the clicking player's feet position
    pub player_pos: [f64; 3],
    /// BlockPlaceContext.replaceClicked (filled in by `PlaceCtx::resolve`)
    pub replace_clicked: bool,
}

impl PlaceCtx {
    /// BlockPlaceContext constructor: decides whether the clicked block itself is replaced.
    pub fn resolve(mut self, w: &World) -> PlaceCtx {
        self.replace_clicked = true; // (canBeReplaced implementations ask the context; start from the plain reading)
        let st = w.get(self.hit_pos);
        self.replace_clicked = place::can_be_replaced(w, st, self.hit_pos, &self);
        self
    }
    /// BlockPlaceContext.getClickedPos
    pub fn pos(&self) -> P { if self.replace_clicked { self.hit_pos } else { self.hit_pos.rel(self.face) } }
    /// UseOnContext.getHorizontalDirection (Entity.getDirection)
    pub fn horizontal_direction(&self) -> Dir { from_yrot(self.yaw as f64) }
    /// BlockPlaceContext.at
    pub fn at(&self, w: &World, pos: P, dir: Dir) -> PlaceCtx {
        let mut c = self.clone();
        c.hit_pos = pos;
        c.face = dir;
        c.hit = [pos.x as f64 + 0.5 + STEP[dir as usize].0 as f64 * 0.5, pos.y as f64 + 0.5 + STEP[dir as usize].1 as f64 * 0.5, pos.z as f64 + 0.5 + STEP[dir as usize].2 as f64 * 0.5];
        c.inside = false;
        c.resolve(w)
    }
}

/// Result of a right click on a block (InteractionResult).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    /// the click was used up (SUCCESS / CONSUME ...): no item use follows
    Consume,
    /// nothing happened (PASS): the held item is used on the block next
    Pass,
    /// useItemOn only: try the empty-hand interaction
    TryEmptyHand,
    /// FAIL: nothing happens and nothing follows
    Fail,
}

/// The clicking player as block interactions see it.
#[derive(Clone, Debug)]
pub struct Clicker {
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub sneak: bool,
    pub item: u16,
    /// set by the rules when the click used up one of the held item (the caller takes it from the inventory)
    pub consumed: bool,
}

// ---------------------------------------------------------------------------------------------- placement
/// The whole of BlockItem.place up to (not including) the block write: where the block would go and as what.
pub fn predict_place(w: &World, ctx: &PlaceCtx) -> Option<(P, S)> { place::predict_place(w, ctx) }
/// BlockState.canSurvive
pub fn can_survive(w: &World, s: S, pos: P) -> bool { place::can_survive(w, s, pos) }
/// BlockState.isFaceSturdy for a support type (0 FULL, 1 CENTER, 2 RIGID), from the block's support shape.
pub fn face_sturdy(s: S, dir: Dir, kind: u8) -> bool { place::face_sturdy(s, dir, kind) }
/// Index (into `t().shapes`) of the collision shape the state has for the "is the space free of entities" test of a placement.
pub fn placement_collision_shape(s: S) -> u32 { place::placement_collision_shape(s) }

// ---------------------------------------------------------------------------------------------- dynamics
/// The block kinds whose `update_shape` can do anything: every kind named in `redstone::update_shape` or
/// `basic::update_shape`. A shape update that lands on any other kind returns the state as it is, and the neighbour
/// updater (`World::run_next`) does not call the rule at all. KEEP IN STEP: a kind that gets an arm there must be added
/// here (MCSIM_UPDCHECK=1 calls the rules all the same and panics if one of the skipped calls does anything).
pub fn shape_reacts(k: crate::kinds::Kind) -> bool {
    use crate::kinds::Kind as K;
    matches!(k,
        K::AmethystClusterBlock | K::AnvilBlock | K::AttachedStemBlock | K::AzaleaBlock | K::BambooSaplingBlock | K::BambooStalkBlock
        | K::BannerBlock | K::BedBlock | K::BellBlock | K::BigDripleafBlock | K::BigDripleafStemBlock | K::BrushableBlock | K::BushBlock
        | K::ButtonBlock | K::CactusBlock | K::CactusFlowerBlock | K::CakeBlock | K::CampfireBlock | K::CandleCakeBlock | K::CarpetBlock
        | K::CaveVinesBlock | K::CaveVinesPlantBlock | K::CeilingHangingSignBlock | K::ChestBlock | K::ChorusFlowerBlock | K::ChorusPlantBlock
        | K::CocoaBlock | K::ComparatorBlock | K::ConcretePowderBlock | K::CoralBlock | K::CoralFanBlock | K::CoralPlantBlock | K::CoralWallFanBlock
        | K::CreakingHeartBlock | K::CropBlock | K::DirtPathBlock | K::DoorBlock | K::DoublePlantBlock | K::DragonEggBlock | K::DryVegetationBlock
        | K::FallingBlock | K::FarmlandBlock | K::FenceBlock | K::FenceGateBlock | K::FireBlock | K::FireflyBushBlock | K::FlowerBedBlock
        | K::FlowerBlock | K::FlowerPotBlock | K::FrogspawnBlock | K::GlowLichenBlock | K::GrindstoneBlock | K::HangingMossBlock
        | K::HangingRootsBlock | K::HugeMushroomBlock | K::IronBarsBlock | K::KelpBlock | K::KelpPlantBlock | K::LadderBlock | K::LanternBlock
        | K::LeafLitterBlock | K::LeavesBlock | K::LeverBlock | K::LilyPadBlock | K::MangrovePropaguleBlock | K::MossyCarpetBlock | K::MultifaceBlock
        | K::MushroomBlock | K::NetherFungusBlock | K::NetherRootsBlock | K::NetherSproutsBlock | K::NetherWartBlock | K::NoteBlock
        | K::ObserverBlock | K::PistonHeadBlock | K::PitcherCropBlock | K::PointedDripstoneBlock | K::PotentSulfurBlock | K::PressurePlateBlock
        | K::RedStoneWireBlock | K::RedstoneTorchBlock | K::RedstoneWallTorchBlock | K::RepeaterBlock | K::SaplingBlock | K::ScaffoldingBlock
        | K::SculkVeinBlock | K::SeaPickleBlock | K::SeagrassBlock | K::SmallDripleafBlock | K::SnowLayerBlock | K::SnowyBlock | K::SporeBlossomBlock
        | K::StairBlock | K::StandingSignBlock | K::StemBlock | K::SugarCaneBlock | K::SulfurSpikeBlock | K::SweetBerryBushBlock | K::TallFlowerBlock
        | K::TallGrassBlock | K::TallSeagrassBlock | K::TorchBlock | K::TorchflowerCropBlock | K::TripWireBlock | K::TripWireHookBlock
        | K::TwistingVinesBlock | K::TwistingVinesPlantBlock | K::VineBlock | K::WallBannerBlock | K::WallBlock | K::WallHangingSignBlock
        | K::WallSignBlock | K::WallTorchBlock | K::WeepingVinesBlock | K::WeepingVinesPlantBlock | K::WeightedPressurePlateBlock)
}
/// The same for `neighbor_changed` (`redstone::neighbor_changed`, `basic::neighbor_changed`).
pub fn neighbor_reacts(k: crate::kinds::Kind) -> bool {
    use crate::kinds::Kind as K;
    matches!(k,
        K::BellBlock | K::BigDripleafBlock | K::ComparatorBlock | K::CopperBulbBlock | K::CrafterBlock | K::DetectorRailBlock | K::DispenserBlock
        | K::DoorBlock | K::FenceGateBlock | K::HopperBlock | K::NoteBlock | K::PistonBaseBlock | K::PistonHeadBlock | K::PoweredRailBlock
        | K::RailBlock | K::RedStoneWireBlock | K::RedstoneLampBlock | K::RedstoneTorchBlock | K::RedstoneWallTorchBlock | K::RepeaterBlock
        | K::ShelfBlock | K::SkullBlock | K::TntBlock | K::TrapDoorBlock | K::WallSkullBlock)
}
pub fn update_shape(w: &mut World, s: S, pos: P, dir: Dir, npos: P, ns: S) -> S {
    if let Some(r) = redstone::update_shape(w, s, pos, dir, npos, ns) { return r; }
    basic::update_shape(w, s, pos, dir, npos, ns)
}
pub fn neighbor_changed(w: &mut World, s: S, pos: P, from_block: u16, moved: bool) {
    if redstone::neighbor_changed(w, s, pos, from_block, moved) { return; }
    basic::neighbor_changed(w, s, pos, from_block, moved);
}
pub fn on_place(w: &mut World, s: S, pos: P, old: S, moved: bool) {
    if redstone::on_place(w, s, pos, old, moved) { return; }
    basic::on_place(w, s, pos, old, moved);
}
pub fn affect_neighbors_after_removal(w: &mut World, old: S, pos: P, moved: bool) {
    if redstone::affect_neighbors_after_removal(w, old, pos, moved) { return; }
    basic::affect_neighbors_after_removal(w, old, pos, moved);
}
pub fn update_indirect_neighbour_shapes(w: &mut World, s: S, pos: P, flags: u32, limit: i32) {
    redstone::update_indirect_neighbour_shapes(w, s, pos, flags, limit);
}
pub fn tick(w: &mut World, s: S, pos: P) {
    if redstone::tick(w, s, pos) { return; }
    basic::tick(w, s, pos);
}
pub fn trigger_event(w: &mut World, s: S, pos: P, b0: u8, b1: u8) -> bool {
    if let Some(r) = redstone::trigger_event(w, s, pos, b0, b1) { return r; }
    basic::trigger_event(w, s, pos, b0, b1)
}
/// Block.setPlacedBy: what a freshly placed block does beyond its own cell (upper door half, bed head ...).
pub fn set_placed_by(w: &mut World, pos: P, s: S, ctx: &PlaceCtx) {
    if redstone::set_placed_by(w, pos, s, ctx) { return; }
    basic::set_placed_by(w, pos, s, ctx);
}
/// Block.playerWillDestroy (before the block is removed by a player breaking it)
pub fn player_will_destroy(w: &mut World, pos: P, s: S) { basic::player_will_destroy(w, pos, s); }
/// Block.destroy (after the removal)
pub fn destroyed(w: &mut World, pos: P, s: S) { basic::destroyed(w, pos, s); }
/// BlockState.attack (left click starts)
pub fn attack(w: &mut World, s: S, pos: P) {
    if redstone::attack(w, s, pos) { return; }
    basic::attack(w, s, pos);
}
/// BlockState.useItemOn
pub fn use_item_on(w: &mut World, s: S, pos: P, face: Dir, hit: [f64; 3], who: &mut Clicker) -> Use {
    if let Some(r) = redstone::use_item_on(w, s, pos, face, hit, who) { return r; }
    basic::use_item_on(w, s, pos, face, hit, who)
}
/// BlockState.useWithoutItem
pub fn use_without_item(w: &mut World, s: S, pos: P, face: Dir, hit: [f64; 3], who: &mut Clicker) -> Use {
    if let Some(r) = redstone::use_without_item(w, s, pos, face, hit, who) { return r; }
    basic::use_without_item(w, s, pos, face, hit, who)
}
/// Entity standing on / inside blocks (Block.stepOn, BlockState.entityInside) for the player, once per tick.
pub fn step_on(w: &mut World, s: S, pos: P, careful: bool) {
    if redstone::step_on(w, s, pos) { return; }
    basic::step_on(w, s, pos, careful);
}
pub fn entity_inside(w: &mut World, s: S, pos: P) {
    if redstone::entity_inside(w, s, pos) { return; }
    basic::entity_inside(w, s, pos);
}
pub fn has_analog_output(s: S) -> bool { redstone::has_analog_output(s) }
pub fn update_neighbour_for_output_signal(w: &mut World, pos: P, block: u16) { redstone::update_neighbour_for_output_signal(w, pos, block); }
/// Level.hasNeighborSignal
pub fn has_neighbor_signal(w: &World, pos: P) -> bool { redstone::has_neighbor_signal(w, pos) }
/// BlockState.shouldChangedStateKeepBlockEntity
pub fn keeps_block_entity(old: S, new: S) -> bool { basic::keeps_block_entity(old, new) }
pub fn is_rail(s: S) -> bool {
    use crate::kinds::Kind::*;
    matches!(kind_of(s), RailBlock | PoweredRailBlock | DetectorRailBlock)
}
