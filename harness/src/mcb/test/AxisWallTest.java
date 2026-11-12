package mcb.test;

import java.nio.file.Paths;
import mcb.build.StateInfo;
import mcb.env.Env;
import mcb.run.BuildRun;
import mcb.schem.Schem;
import net.minecraft.core.Direction;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.RotatedPillarBlock;

/** A one-wide wall whose top course is logs lying across it: every one of them needs a face that no neighbour offers. */
public class AxisWallTest {
    public static void main(String[] a) throws Exception {
        Env env = Env.boot(Paths.get("/tmp/mcb-axis"));
        StateInfo.init();
        int stone = Block.getId(Blocks.STONE_BRICKS.defaultBlockState());
        int logX = Block.getId(Blocks.DEEPSLATE.defaultBlockState().setValue(RotatedPillarBlock.AXIS, Direction.Axis.X));
        int sx = 1, sy = 4, sz = 5;
        char[] ids = new char[sx * sy * sz];
        for (int y = 0; y < 3; y++) for (int z = 0; z < sz; z++) ids[(y * sz + z) * sx] = (char) stone;
        for (int z = 0; z < sz; z++) ids[(3 * sz + z) * sx] = (char) logX;
        Schem s = new Schem(sx, sy, sz, ids);
        BuildRun.Outcome o = BuildRun.run(env, s, 100000, true);
        System.out.println("RESULT " + o);
        System.out.println(o.judge.badByBlock + " " + o.skipReasons + " " + o.navStats);
        System.exit(0);
    }
}
