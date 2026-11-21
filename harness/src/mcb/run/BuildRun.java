package mcb.run;

import mcb.bot.Builder;
import mcb.bot.Grid;
import mcb.build.Judge;
import mcb.build.Site;
import mcb.build.StateInfo;
import mcb.build.Target;
import mcb.env.Client;
import mcb.env.Env;
import mcb.schem.Schem;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;

/** One build from start to verdict, in a fresh patch of the world. */
public final class BuildRun {
    /** 48 hours of game time at 20 ticks per second. */
    public static final long TICK_LIMIT = 48L * 3600 * 20;

    public static final class Outcome {
        public Judge.Result judge;
        public String status = "";
        public long ticks, placed, broken, skipped, trips, scaffoldLeft, scaffoldPlaced, replans, wallMs;
        public long targetBlocks;
        public String skipReasons = "", navStats = "", metrics = "";
        @Override public String toString() {
            return String.format("%s | %s | blocks=%d ticks=%d (%.1f t/block) placed=%d broken=%d skipped=%d trips=%d scaffold=%d left=%d replans=%d wall=%.1fs | %s",
                judge != null && judge.pass ? "PASS" : "FAIL", status, targetBlocks, ticks, targetBlocks == 0 ? 0 : (double) ticks / targetBlocks, placed, broken, skipped, trips,
                scaffoldPlaced, scaffoldLeft, replans, wallMs / 1000.0, judge);
        }
    }

    private static int nextX = 0;
    private static String setupDetail = "";

    private static mcb.env.Oracle oracle;
    public static long wallLimitMs = 0;

    public static Outcome run(Env env, Schem schem, long tickLimit, boolean verbose) {
        if (oracle == null) oracle = new mcb.env.Oracle(env);
        StateInfo.init();
        long t0 = System.currentTimeMillis();
        Outcome out = new Outcome();
        Target target = new Target(schem);
        out.targetBlocks = target.blocks;
        // every build uses the same site; it is wiped clean after each run
        int ox = 64, oz = 64;
        long ta = System.currentTimeMillis();
        Site site = new Site(env, target, ox, oz);
        long tb = System.currentTimeMillis();
        if (!site.fitsWorld()) { out.status = "too tall for the world"; out.judge = Judge.judge(target, new char[target.ids.length]); return out; }
        int scaffoldCount = (int) Math.min(64L * 54 * 20, 64 * 27 + target.blocks * 3 + 64L * target.sy * 2);
        int towerCount = (int) Math.min(64L * 54 * 20, 64 * 27 + target.blocks * 3 + 64L * target.sy * 8);
        site.setUp(scaffoldCount, towerCount);
        int depotDepth0 = Site.ROW_GAP + site.depotRows * Site.ROW_PITCH + 2;
        int marginX0 = Math.max(6, site.depotCols * 2 - target.sx + 4);
        env.pinSite(ox - marginX0, oz - depotDepth0, ox + target.sx + marginX0, oz + target.sz + 6, ox, oz - 2);
        long tc = System.currentTimeMillis();
        // MCB_SEED=k: the bot starts k blocks further along the front of the build. Nothing else changes, but the
        // whole run takes another course: the way to measure how much of a verdict is the build and how much is chance.
        int seed = System.getenv("MCB_SEED") == null ? 0 : Integer.parseInt(System.getenv("MCB_SEED"));
        Client client = env.join("Bot" + (ox / 64), ox + 0.5 + Math.floorMod(seed, Math.max(1, Math.min(target.sx, 12))), site.oy, oz - 2 + 0.5);
        long td = System.currentTimeMillis();
        setupDetail = "target=" + (ta - t0) + " site=" + (tb - ta) + " chests+pin=" + (tc - tb) + " join=" + (td - tc);
        client.collectPacketStats = verbose;
        for (int i = 0; i < 3; i++) client.tick();
        client.clearChanges();
        int depotDepth = Site.ROW_GAP + site.depotRows * Site.ROW_PITCH + 2;
        int marginX = Math.max(6, site.depotCols * 2 - target.sx + 4);
        int ground = Block.getId(Blocks.GRASS_BLOCK.defaultBlockState());
        int top = Math.min(8, env.level.getMaxY() - (site.oy + target.sy));
        Grid grid = new Grid(ox, site.oy, oz, target.sx, target.sy, target.sz, target.ids, marginX, depotDepth, 6, top, ground);
        long tSetup = System.currentTimeMillis() - t0;
        Builder b = new Builder(client, grid, site, oracle);
        b.maxTicks = tickLimit;
        b.wallLimitMs = wallLimitMs;
        b.verbose = verbose;
        try {
            b.run();
        } catch (RuntimeException e) {
            b.status = "crash: " + e;
            if (verbose) e.printStackTrace();
        }
        long tRun = System.currentTimeMillis() - t0 - tSetup;
        out.status = b.status;
        out.skipReasons = b.skipReasons.toString();
        out.navStats = b.navStats();
        out.metrics = b.metrics();
        out.ticks = b.ticks; out.placed = b.placed; out.broken = b.broken; out.skipped = b.skippedCells; out.trips = b.trips;
        out.scaffoldLeft = b.scaffoldLeft(); out.scaffoldPlaced = b.scaffoldPlaced; out.replans = b.replans;
        out.navStats += " busy[" + env.busyReport() + "] ticks full/idle=" + env.fullTicks + "/" + env.idleTicks + " loadedChunks=" + env.level.getChunkSource().getLoadedChunksCount() + " entities=" + com.google.common.collect.Iterables.size(env.level.getAllEntities());
        for (int i = 0; i < 5; i++) env.tickServer();
        out.judge = Judge.judge(target, site.snapshot(), env.level);
        if (verbose) System.out.println("PACKETS " + client.packetStats + " tracked=" + env.level.getChunkSource().chunkMap.isChunkTracked(client.player, (ox + 4) >> 4, 0));
        if (verbose) {
            // where the bot's picture of the world and the world disagree
            int diff = 0;
            net.minecraft.core.BlockPos.MutableBlockPos p = new net.minecraft.core.BlockPos.MutableBlockPos();
            for (int i = 0; i < grid.cur.length && diff < 12; i++) {
                int x = grid.xOf(i), y = grid.yOf(i), z = grid.zOf(i);
                if (y < grid.oy) continue;
                int w = Block.getId(env.level.getBlockState(p.set(x, y, z)));
                if (StateInfo.air[w]) w = 0;
                if (w != grid.cur[i] && !(StateInfo.state[w].getBlock() == Blocks.CHEST)) { diff++; System.out.println("MIRROR MISMATCH at " + (x - ox) + "," + (y - site.oy) + "," + (z - oz) + " world=" + StateInfo.name(w) + " bot=" + StateInfo.name(grid.cur[i])); }
            }
        }
        env.server.getPlayerList().remove(client.player);
        long wiped = env.wipe(grid.x0, site.oy, grid.z0, grid.x0 + grid.nx - 1, site.oy + grid.ny, grid.z0 + grid.nz - 1);
        for (int i = 0; i < 3; i++) env.tickServer();
        env.unpinSite();
        out.navStats += " wiped=" + wiped;
        out.wallMs = System.currentTimeMillis() - t0;
        out.navStats += " [" + setupDetail + "] setupMs=" + tSetup + " runMs=" + tRun + " totalMs=" + out.wallMs;
        return out;
    }
}
