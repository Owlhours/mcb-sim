package mcb.test;

import java.nio.file.Paths;
import mcb.env.Client;
import mcb.env.Env;

public class TickCost {
    public static void main(String[] a) throws Exception {
        Env env = Env.boot(Paths.get("/tmp/mcb-tickcost"));
        Client c = env.join("Bot", 1000.5, -60, 1000.5);
        for (int i = 0; i < 200; i++) c.tick();
        if (a.length > 0) {
            int n = 0;
            for (int cx = (1000 - 210) >> 4; cx <= (1000 + 210) >> 4; cx++) for (int cz = (1000 - 60) >> 4; cz <= (1000 + 60) >> 4; cz++) { env.level.setChunkForced(cx, cz, true); n++; }
            System.out.println("forced " + n + " chunks");
            for (int i = 0; i < 400; i++) c.tick();
        }
        java.lang.reflect.Field pf = c.player.connection.chunkSender.getClass().getDeclaredField("pendingChunks");
        pf.setAccessible(true);
        it.unimi.dsi.fastutil.longs.LongSet pending = (it.unimi.dsi.fastutil.longs.LongSet) pf.get(c.player.connection.chunkSender);
        boolean skipSend = a.length > 1;
        int[] spans = {4, 40, 200};
        for (int rep = 0; rep < 2; rep++) for (int span : spans) {
            long t0 = System.nanoTime();
            int N = 20000;
            double x0 = 1000.5;
            float yaw = -90;
            for (int i = 0; i < N; i++) {
                if (c.x() > x0 + span) yaw = 90; else if (c.x() < x0 - span) yaw = -90;
                c.look(yaw, 0);
                c.keys(true, false, false, false, false, false, false);
                if (skipSend) pending.clear();
                c.tick();
            }
            System.out.printf("span +-%d blocks: %.1f us/tick%n", span, (System.nanoTime() - t0) / 1e3 / N);
        }
        long t0 = System.nanoTime();
        c.releaseKeys();
        for (int i = 0; i < 20000; i++) c.tick();
        System.out.printf("standing still: %.1f us/tick%n", (System.nanoTime() - t0) / 1e3 / 20000);
        System.exit(0);
    }
}
