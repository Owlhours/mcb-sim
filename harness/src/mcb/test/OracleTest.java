package mcb.test;

import java.nio.file.Paths;
import java.util.TreeMap;
import mcb.build.StateInfo;
import mcb.env.Env;
import mcb.env.ServerOracle;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.Items;

public class OracleTest {
    public static void main(String[] a) throws Exception {
        Env env = Env.boot(Paths.get("/tmp/mcb-oracle"));
        StateInfo.init();
        ServerOracle o = new ServerOracle(env);
        String[] faces = {"down", "up", "north", "south", "west", "east"};
        for (Item it : new Item[] {Items.OAK_STAIRS, Items.STONE_SLAB, Items.OAK_LOG, Items.TORCH, Items.OAK_DOOR, Items.PISTON, Items.LANTERN, Items.OAK_TRAPDOOR, Items.POPPY, Items.CHEST, Items.STONE}) {
            long t0 = System.nanoTime();
            char[] t = o.table(mcb.mc.BlockFacts.itemId(it));
            long t1 = System.nanoTime();
            TreeMap<String, Integer> states = new TreeMap<>();
            for (char c : t) if (c != 0) states.merge(StateInfo.name(StateInfo.canon[c]), 1, Integer::sum);
            System.out.println(it + " (" + (t1 - t0) / 1000000 + " ms): " + states.size() + " distinct results: " + (states.size() > 12 ? states.keySet().stream().limit(12).toList() + "..." : states));
        }
        // a few spot checks: stairs clicked on the top face while looking north, east; on the upper half of a side face
        char[] st = o.table(mcb.mc.BlockFacts.itemId(Items.OAK_STAIRS));
        for (float yaw : new float[] {180, -90, 0, 90}) System.out.println("stairs, top face, yaw " + yaw + " -> " + StateInfo.name(st[ServerOracle.index(1, 0, ServerOracle.yawClass(yaw + 3), ServerOracle.pitchClass(40), false)]));
        System.out.println("stairs, north face upper half -> " + StateInfo.name(st[ServerOracle.index(2, 2, ServerOracle.yawClass(3), ServerOracle.pitchClass(0), false)]));
        System.out.println("stairs, north face lower half -> " + StateInfo.name(st[ServerOracle.index(2, 0, ServerOracle.yawClass(3), ServerOracle.pitchClass(0), false)]));
        System.exit(0);
    }
}
