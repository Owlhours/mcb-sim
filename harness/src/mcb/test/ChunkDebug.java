package mcb.test;

import java.lang.reflect.Field;
import java.nio.file.Paths;
import mcb.env.Client;
import mcb.env.Env;

public class ChunkDebug {
    static Object get(Object o, String f) throws Exception { Field x = o.getClass().getDeclaredField(f); x.setAccessible(true); return x.get(o); }
    public static void main(String[] a) throws Exception {
        Env env = Env.boot(Paths.get("/tmp/mcb-chunkdbg"));
        Client c = env.join("Bot", 64.5, -60, -1.5);
        c.collectPacketStats = true;
        for (int i = 0; i < 6; i++) {
            c.tick();
            Object cs = c.player.connection.chunkSender;
            System.out.println("tick " + i + " pending=" + ((java.util.Collection<?>) get(cs, "pendingChunks")).size() + " unacked=" + get(cs, "unacknowledgedBatches") + " maxUnacked=" + get(cs, "maxUnacknowledgedBatches")
                + " quota=" + get(cs, "batchQuota") + " desired=" + get(cs, "desiredChunksPerTick") + " view=" + c.player.getChunkTrackingView() + " chunkPos=" + c.player.chunkPosition() + " packets=" + c.packetStats);
        }
        System.exit(0);
    }
}
