package mcb.test;

import java.nio.file.Paths;
import mcb.build.StateInfo;
import mcb.env.Env;
import mcb.headless.BuildRun;
import mcb.schem.Schem;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;

/** A post with a beam sticking out at the top, and a second floating beam: needs leaning out and bridging. */
public class BeamTest {
    public static void main(String[] a) throws Exception {
        Env env = Env.boot(Paths.get("/tmp/mcb-beam"));
        StateInfo.init();
        int stone = Block.getId(Blocks.STONE_BRICKS.defaultBlockState()), planks = Block.getId(Blocks.OAK_PLANKS.defaultBlockState());
        int sx = 8, sy = 6, sz = 5;
        char[] ids = new char[sx * sy * sz];
        for (int y = 0; y < 5; y++) ids[(y * sz + 2) * sx + 0] = (char) stone;          // post at x=0,z=2
        for (int x = 1; x < 6; x++) ids[(4 * sz + 2) * sx + x] = (char) planks;          // beam from the post top
        for (int x = 3; x < 8; x++) ids[(5 * sz + 4) * sx + x] = (char) planks;          // a beam attached to nothing
        Schem s = new Schem(sx, sy, sz, ids);
        BuildRun.Outcome o = BuildRun.run(env, s, 200000, true);
        System.out.println("RESULT " + o);
        System.out.println(o.judge.badByBlock + " " + o.skipReasons + " " + o.navStats);
        System.exit(0);
    }
}
