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

    private Env(GameTestServer server, MethodHandle tickServer) throws Exception {
        this.waitUntilNextTick = GameTestServer.class.getDeclaredMethod("waitUntilNextTick");
        this.waitUntilNextTick.setAccessible(true);
        this.server = server;
        this.level = server.overworld();
        this.tickServer = tickServer;
    }

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
        server.getPlayerList().setViewDistance(6);
        server.getPlayerList().setSimulationDistance(4);
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
        BotPlayer player = new BotPlayer(server, level, profile);
        FakeConnection conn = new FakeConnection();
        Client client = new Client(this, player, conn);
        server.getPlayerList().placeNewPlayer(conn, player, CommonListenerCookie.createInitial(profile, false));
        player.setGameMode(GameType.SURVIVAL);
        player.getInventory().clearContent();
        client.attach();
        client.adminTeleport(x, y, z);
        return client;
    }
}
