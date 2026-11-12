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
        @Override public String toString() {
            return String.format("%s | %s | blocks=%d ticks=%d (%.1f t/block) placed=%d broken=%d skipped=%d trips=%d scaffold=%d left=%d replans=%d wall=%.1fs | %s",
                judge != null && judge.pass ? "PASS" : "FAIL", status, targetBlocks, ticks, targetBlocks == 0 ? 0 : (double) ticks / targetBlocks, placed, broken, skipped, trips,
                scaffoldPlaced, scaffoldLeft, replans, wallMs / 1000.0, judge);
        }
    }

    private static int nextX = 0;

    public static Outcome run(Env env, Schem schem, long tickLimit, boolean verbose) {
        StateInfo.init();
        long t0 = System.currentTimeMillis();
        Outcome out = new Outcome();
        Target target = new Target(schem);
        out.targetBlocks = target.blocks;
        // every build gets its own untouched area, far from the previous one
        int ox = nextX + 64, oz = 0;
        nextX = ox + target.sx + 64 + 512;
        Site site = new Site(env, target, ox, oz);
        if (!site.fitsWorld()) { out.status = "too tall for the world"; out.judge = Judge.judge(target, new char[target.ids.length]); return out; }
        int scaffoldCount = (int) Math.min(64L * 54 * 6, 64 * 8 + target.blocks / 2 + 4L * target.sy * 4);
        site.setUp(scaffoldCount);
        Client client = env.join("Bot" + (ox / 64), ox + 0.5, site.oy, oz - 2 + 0.5);
        client.collectPacketStats = verbose;
        for (int i = 0; i < 3; i++) client.tick();
        client.clearChanges();
        int depotDepth = Site.ROW_GAP + site.depotRows * Site.ROW_PITCH + 2;
        int marginX = Math.max(6, site.depotCols * 2 - target.sx + 4);
        int ground = Block.getId(Blocks.GRASS_BLOCK.defaultBlockState());
        int top = Math.min(8, env.level.getMaxY() - (site.oy + target.sy));
        Grid grid = new Grid(ox, site.oy, oz, target.sx, target.sy, target.sz, target.ids, marginX, depotDepth, 6, top, ground);
        Builder b = new Builder(client, grid, site);
        b.maxTicks = tickLimit;
        b.verbose = verbose;
        try {
            b.run();
        } catch (RuntimeException e) {
            b.status = "crash: " + e;
            if (verbose) e.printStackTrace();
        }
        out.status = b.status;
        out.ticks = b.ticks; out.placed = b.placed; out.broken = b.broken; out.skipped = b.skippedCells; out.trips = b.trips;
        out.scaffoldLeft = b.scaffoldLeft(); out.scaffoldPlaced = b.scaffoldPlaced; out.replans = b.replans;
        for (int i = 0; i < 5; i++) env.tickServer();
        out.judge = Judge.judge(target, site.snapshot());
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
        out.wallMs = System.currentTimeMillis() - t0;
        return out;
    }
}
