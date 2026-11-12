package mcb.test;

import java.nio.file.Paths;
import mcb.build.StateInfo;
import mcb.env.Env;
import mcb.run.BuildRun;
import mcb.schem.Schem;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;

public class FirstBuild {
    public static void main(String[] a) throws Exception {
        Env env = Env.boot(Paths.get("/tmp/mcb-first"));
        StateInfo.init();
        int stone = Block.getId(Blocks.STONE_BRICKS.defaultBlockState()), planks = Block.getId(Blocks.OAK_PLANKS.defaultBlockState()), glass = Block.getId(Blocks.GLASS.defaultBlockState());
        // a small house: 7 x 5 x 6, plank floor and roof, stone walls with a door gap and windows
        int sx = 7, sy = 5, sz = 6;
        char[] ids = new char[sx * sy * sz];
        for (int y = 0; y < sy; y++) for (int z = 0; z < sz; z++) for (int x = 0; x < sx; x++) {
            boolean wall = x == 0 || x == sx - 1 || z == 0 || z == sz - 1;
            int id = 0;
            if (y == 0 || y == sy - 1) id = planks;
            else if (wall) {
                id = stone;
                if (y == 2 && (x == 3 || z == 3)) id = glass;
                if (x == 2 && z == 0 && (y == 1 || y == 2)) id = 0; // doorway
            }
            ids[(y * sz + z) * sx + x] = (char) id;
        }
        Schem s = new Schem(sx, sy, sz, ids);
        BuildRun.Outcome o = BuildRun.run(env, s, BuildRun.TICK_LIMIT, true);
        System.out.println("RESULT " + o);
        System.out.println(o.judge.badByBlock);
        System.exit(0);
    }
}
