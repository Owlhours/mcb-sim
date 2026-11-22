package mcb.test;

import java.nio.file.Paths;
import mcb.build.StateInfo;
import mcb.env.Env;
import mcb.headless.BuildRun;
import mcb.schem.Schem;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;

/** Three stalks of bamboo of height 3, 2 and 1 on a strip of dirt. */
public class BambooTest {
    public static void main(String[] a) throws Exception {
        Env env = Env.boot(Paths.get("/tmp/mcb-bamboo"));
        StateInfo.init();
        int dirt = Block.getId(Blocks.DIRT.defaultBlockState()), bamboo = Block.getId(Blocks.BAMBOO.defaultBlockState());
        int sx = 5, sy = 4, sz = 1;
        char[] ids = new char[sx * sy * sz];
        for (int x = 0; x < sx; x++) ids[x] = (char) dirt;
        for (int k = 0; k < 3; k++) for (int y = 1; y <= 3 - k; y++) ids[(y * sz) * sx + k * 2] = (char) bamboo;
        BuildRun.Outcome o = BuildRun.run(env, new Schem(sx, sy, sz, ids), 200000, true);
        System.out.println("RESULT " + o);
        System.out.println(o.judge.badByBlock + " " + o.skipReasons);
        System.exit(0);
    }
}
