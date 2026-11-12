package mcb.env;

import com.mojang.authlib.GameProfile;
import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.lang.invoke.MethodType;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Comparator;
import java.util.Optional;
import java.util.UUID;
import java.util.function.BooleanSupplier;
import net.minecraft.SharedConstants;
import net.minecraft.gametest.framework.GameTestServer;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.network.CommonListenerCookie;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.world.Difficulty;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.storage.LevelStorageSource;

/**
 * The game: an unmodified Minecraft 26.2 server (the headless "game test" flavour: flat world, no network)
 * created and ticked on the calling thread. This class is the administrator's side (world setup, ticking);
 * the bot never touches it directly and only talks to {@link Client}.
 */
public final class Env {
    public final GameTestServer server;
    public final ServerLevel level;
    private final MethodHandle tickServer;
    private final Method waitUntilNextTick;
    private static final BooleanSupplier NO_TIME = () -> false;
    public long serverTicks;
    /** Ticks in which the whole server was run / in which only the player needed running (see {@link #tick}). */
    public long fullTicks, idleTicks;
    /** Set to false to run the complete server tick every time. */
    public boolean skipIdleWorld = true;
    private final java.util.Collection<?> blockEvents, blockEntityTickers, pendingBlockEntityTickers, holdersToBroadcast;
    private final java.lang.reflect.Field activeEntities;
    private final Object entityTickList;
    private int forceFull = 40;
    private long lastSection = Long.MIN_VALUE;

    private Env(GameTestServer server, MethodHandle tickServer) throws Exception {
        this.waitUntilNextTick = GameTestServer.class.getDeclaredMethod("waitUntilNextTick");
        this.waitUntilNextTick.setAccessible(true);
        this.server = server;
        this.level = server.overworld();
        this.tickServer = tickServer;
        this.blockEvents = (java.util.Collection<?>) field(ServerLevel.class, "blockEvents").get(level);
        this.blockEntityTickers = (java.util.Collection<?>) field(net.minecraft.world.level.Level.class, "blockEntityTickers").get(level);
        this.pendingBlockEntityTickers = (java.util.Collection<?>) field(net.minecraft.world.level.Level.class, "pendingBlockEntityTickers").get(level);
        this.holdersToBroadcast = (java.util.Collection<?>) field(net.minecraft.server.level.ServerChunkCache.class, "chunkHoldersToBroadcast").get(level.getChunkSource());
        this.entityTickList = field(ServerLevel.class, "entityTickList").get(level);
        this.activeEntities = field(net.minecraft.world.level.entity.EntityTickList.class, "active");
    }

    private static java.lang.reflect.Field field(Class<?> c, String name) throws NoSuchFieldException {
        java.lang.reflect.Field f = c.getDeclaredField(name);
        f.setAccessible(true);
        return f;
    }

    /**
     * One game tick after the player's own tick. When nothing in the world is pending - no scheduled block or
     * fluid ticks, no block events, no entity other than the player, no block change
     * waiting to be sent, the player still in the same chunk section - a server tick would change nothing but
     * its clocks, so only the player's server-side tick and the block entities' ticks (furnaces, hoppers ...) are
     * run, as the level's own loops would run them.
     * In every other case, and at least every 20th tick, the complete, unmodified server tick runs.
     */
    public void tick(net.minecraft.server.level.ServerPlayer player) {
        if (!pinned) {
            long section = net.minecraft.core.SectionPos.asLong(player.blockPosition());
            if (section != lastSection) { lastSection = section; forceFull = Math.max(forceFull, 6); }
        }
        boolean idle = skipIdleWorld && forceFull <= 0 && worldIdle();
        if (!idle) {
            if (forceFull > 0) forceFull--;
            sinceFull = 0;
            fullTicks++;
            tickServer();
            // stay with full ticks until everything that was set in motion has run its course
            if (forceFull == 0 && scheduledWork()) forceFull = 1;
            return;
        }
        if (++sinceFull >= 20) { sinceFull = 0; fullTicks++; tickServer(); if (scheduledWork()) forceFull = 1; return; }
        idleTicks++;
        level.tickNonPassenger(player);
        if (!blockEntityTickers.isEmpty()) {
            // furnaces, hoppers and the like get their tick as usual; if one of them sets something going, full ticks resume
            level.tickBlockEntities();
            if (scheduledWork()) forceFull = 1;
        }
        serverTicks++;
    }
    private int sinceFull;
    private boolean pinned;

    /** Cheap per-tick test; scheduled ticks are looked at only after full ticks (nothing can schedule one in between). */
    private boolean worldIdle() {
        try {
            if (!blockEvents.isEmpty() || !pendingBlockEntityTickers.isEmpty() || !holdersToBroadcast.isEmpty()) return false;
            return ((java.util.Map<?, ?>) activeEntities.get(entityTickList)).size() <= server.getPlayerList().getPlayerCount();
        } catch (IllegalAccessException e) {
            return false;
        }
    }

    /** Why the world is not idle right now (for diagnostics). */
    public String busyReport() {
        try {
            StringBuilder sb = new StringBuilder();
            sb.append("blockEvents=").append(((java.util.Collection<?>) blockEvents).size());
            sb.append(" tickers=").append(((java.util.Collection<?>) blockEntityTickers).size());
            sb.append(" pendingTickers=").append(((java.util.Collection<?>) pendingBlockEntityTickers).size());
            sb.append(" toBroadcast=").append(((java.util.Collection<?>) holdersToBroadcast).size());
            sb.append(" entities=").append(((java.util.Map<?, ?>) activeEntities.get(entityTickList)).size());
            sb.append(" blockTicks=").append(level.getBlockTicks().count()).append(" fluidTicks=").append(level.getFluidTicks().count());
            sb.append(" forceFull=").append(forceFull);
            return sb.toString();
        } catch (Exception e) { return "?" + e; }
    }

    private boolean scheduledWork() { return level.getBlockTicks().count() != 0 || level.getFluidTicks().count() != 0; }

    /** The client did something that can change the world (a click, a container action): run full ticks for a while. */
    public void noteAction() { forceFull = Math.max(forceFull, 2); }

    private final java.util.ArrayList<long[]> pins = new java.util.ArrayList<>();

    /**
     * Keeps the chunks of a build site loaded for the duration of a build and sets the view distance so that the
     * player sees all of it from where it joins (like playing with a large render distance). Then nothing in the
     * chunk system changes as the player walks about. Returns false (and does nothing) if the site is too big
     * for that; the player then gets ordinary moving chunk tracking.
     */
    public boolean pinSite(int minBlockX, int minBlockZ, int maxBlockX, int maxBlockZ, int viewBlockX, int viewBlockZ) {
        int cx0 = (minBlockX >> 4) - 1, cz0 = (minBlockZ >> 4) - 1, cx1 = (maxBlockX >> 4) + 1, cz1 = (maxBlockZ >> 4) + 1;
        int vx = viewBlockX >> 4, vz = viewBlockZ >> 4;
        int radius = Math.max(Math.max(vx - cx0, cx1 - vx), Math.max(vz - cz0, cz1 - vz)) + 1;
        long count = (long) (cx1 - cx0 + 1) * (cz1 - cz0 + 1);
        if (radius > 32 || count > 2500) { unpinSite(); server.getPlayerList().setViewDistance(3); viewDistance = 3; return false; }
        for (int x = cx0; x <= cx1; x++) for (int z = cz0; z <= cz1; z++) { level.setChunkForced(x, z, true); pins.add(new long[] {x, z}); }
        viewDistance = Math.max(3, radius);
        server.getPlayerList().setViewDistance(viewDistance);
        pinned = true;
        forceFull = Math.max(forceFull, 40);
        return true;
    }

    public void unpinSite() {
        for (long[] p : pins) level.setChunkForced((int) p[0], (int) p[1], false);
        pins.clear();
        pinned = false;
        forceFull = Math.max(forceFull, 20);
    }

    public boolean pinned() { return pinned; }

    /**
     * Administrator action between builds: empties a box of the world (everything above the ground becomes air
     * again) so that the same site can be used for the next build. Returns the number of blocks removed.
     */
    public long wipe(int x0, int y0, int z0, int x1, int y1, int z1) {
        long n = 0;
        net.minecraft.core.BlockPos.MutableBlockPos p = new net.minecraft.core.BlockPos.MutableBlockPos();
        net.minecraft.world.level.block.state.BlockState air = net.minecraft.world.level.block.Blocks.AIR.defaultBlockState();
        y1 = Math.min(y1, level.getMaxY());
        for (int cx = x0 >> 4; cx <= x1 >> 4; cx++) for (int cz = z0 >> 4; cz <= z1 >> 4; cz++) {
            net.minecraft.world.level.chunk.LevelChunk chunk = level.getChunk(cx, cz);
            for (int sy = y0 >> 4; sy <= y1 >> 4; sy++) {
                int si = chunk.getSectionIndexFromSectionY(sy);
                if (si < 0 || si >= chunk.getSectionsCount() || chunk.getSection(si).hasOnlyAir()) continue;
                for (int y = Math.max(y0, sy << 4); y <= Math.min(y1, (sy << 4) + 15); y++)
                    for (int z = Math.max(z0, cz << 4); z <= Math.min(z1, (cz << 4) + 15); z++)
                        for (int x = Math.max(x0, cx << 4); x <= Math.min(x1, (cx << 4) + 15); x++) {
                            if (!level.getBlockState(p.set(x, y, z)).isAir()) { level.setBlock(p, air, 2 | 16); n++; }
                        }
            }
        }
        // the ground under the site, should anything have happened to it
        if (y0 - 1 >= level.getMinY()) {
            net.minecraft.world.level.block.state.BlockState ground = null;
            for (int dx = -40; dx <= 40 && ground == null; dx += 8) { var st = level.getBlockState(p.set(x0 - 100 + dx, y0 - 1, z0 - 100)); if (!st.isAir()) ground = st; }
            if (ground != null) {
                for (int z = z0; z <= z1; z++) for (int x = x0; x <= x1; x++) {
                    if (level.getBlockState(p.set(x, y0 - 1, z)) != ground) { level.setBlock(p, ground, 2 | 16); n++; }
                }
            }
        }
        // anything left lying or scheduled there
        for (net.minecraft.world.entity.Entity e : com.google.common.collect.Lists.newArrayList(level.getAllEntities())) {
            if (!(e instanceof net.minecraft.server.level.ServerPlayer)) e.discard();
        }
        busy(10);
        return n;
    }
    int viewDistance = 3;

    /** Asks for complete server ticks for the next n ticks (after administrative changes to the world). */
    public void busy(int n) { forceFull = Math.max(forceFull, n); }

    public static Env boot(Path universe) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        if (Files.exists(universe)) {
            try (var s = Files.walk(universe)) { s.sorted(Comparator.reverseOrder()).forEach(p -> p.toFile().delete()); }
        }
        Files.createDirectories(universe);
        LevelStorageSource.LevelStorageAccess access = LevelStorageSource.createDefault(universe).createAccess("world");
        PackRepository packs = ServerPacksSource.createPackRepository(access);
        // The server normally runs its own thread (MinecraftServer.spin). We make the calling thread the server
        // thread and drive initServer()/tickServer() ourselves so that ticks run back to back, in lockstep with the bot.
        GameTestServer server = GameTestServer.create(Thread.currentThread(), access, packs, Optional.empty(), false, 1);
        Method init = GameTestServer.class.getDeclaredMethod("initServer");
        init.setAccessible(true);
        if (!(boolean) init.invoke(server)) throw new IllegalStateException("initServer failed");
        // MinecraftServer.tickServer, bypassing GameTestServer's override (which only adds the test runner and
        // halts the server once "all tests" are done).
        MethodHandle tick = MethodHandles.privateLookupIn(GameTestServer.class, MethodHandles.lookup())
            .findSpecial(MinecraftServer.class, "tickServer", MethodType.methodType(void.class, BooleanSupplier.class), GameTestServer.class);
        Env env = new Env(server, tick);
        env.applyWorldRules();
        return env;
    }

    /** The fixed, documented environment: survival conditions with the allowances stated in GOAL.md. */
    private void applyWorldRules() {
        GameRules r = level.getGameRules();
        r.set(GameRules.BLOCK_DROPS, false, server);          // broken blocks leave no items ("losing scaffold items is fine"); keeps the world free of item entities
        r.set(GameRules.FALL_DAMAGE, false, server);          // "the player has fall protection"
        r.set(GameRules.RANDOM_TICK_SPEED, 0, server);        // no crop growth / grass spread / leaf decay while building
        r.set(GameRules.ADVANCE_TIME, false, server);
        r.set(GameRules.ADVANCE_WEATHER, false, server);
        r.set(GameRules.SPAWN_MOBS, false, server);
        r.set(GameRules.SPAWN_MONSTERS, false, server);
        r.set(GameRules.SPAWN_PATROLS, false, server);
        r.set(GameRules.SPAWN_PHANTOMS, false, server);
        r.set(GameRules.SPAWN_WANDERING_TRADERS, false, server);
        r.set(GameRules.SPAWN_WARDENS, false, server);
        server.setDifficulty(Difficulty.PEACEFUL, true);      // no hunger drain, no hostile mobs
        server.getPlayerList().setViewDistance(3);
        server.getPlayerList().setSimulationDistance(3);
    }

    public void tickServer() {
        try {
            tickServer.invoke(server, NO_TIME);
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new RuntimeException(t);
        }
        // what the server's own loop does between ticks: run the queued main-thread tasks (chunk loading and
        // delivery, among others). The test server's version does this without sleeping.
        try {
            waitUntilNextTick.invoke(server);
        } catch (ReflectiveOperationException e) {
            throw new RuntimeException(e.getCause() != null ? e.getCause() : e);
        }
        // ... and the chunk system's own queue, which the server only drains when it has spare time in a tick
        // (it decides that from its real-time schedule, which does not exist here)
        int guard = 0;
        while (level.getChunkSource().pollTask() && ++guard < 10000) { }
        serverTicks++;
    }

    /** Adds the bot's player to the world in survival mode with an empty inventory. */
    public Client join(String name, double x, double y, double z) {
        GameProfile profile = new GameProfile(UUID.nameUUIDFromBytes(("mcb:" + name).getBytes()), name);
        BotPlayer player = new BotPlayer(server, level, profile, viewDistance);
        FakeConnection conn = new FakeConnection();
        Client client = new Client(this, player, conn);
        // join where the work is, not at the world spawn (which would load and send the whole spawn area first)
        player.snapTo(x, y, z, 0.0f, 0.0f);
        server.getPlayerList().placeNewPlayer(conn, player, CommonListenerCookie.createInitial(profile, false));
        player.setGameMode(GameType.SURVIVAL);
        player.getInventory().clearContent();
        client.attach();
        client.adminTeleport(x, y, z);
        return client;
    }
}
