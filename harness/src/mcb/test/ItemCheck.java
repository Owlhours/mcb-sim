package mcb.test;

import java.nio.file.Paths;
import mcb.build.StateInfo;
import mcb.env.Env;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;

public class ItemCheck {
    public static void main(String[] a) throws Exception {
        Env.boot(Paths.get("/tmp/mcb-itemcheck"));
        StateInfo.init();
        for (Block b : new Block[] {Blocks.WATER_CAULDRON, Blocks.CAULDRON, Blocks.POTTED_POPPY, Blocks.HAY_BLOCK, Blocks.BROWN_MUSHROOM}) {
            for (var s : b.getStateDefinition().getPossibleStates()) {
                int id = Block.getId(s);
                System.out.println(StateInfo.name(id) + " item=" + StateInfo.item[id] + " count=" + StateInfo.itemCount[id] + " canon=" + StateInfo.name(StateInfo.canon[id]) + " loose=" + StateInfo.name(StateInfo.loose[id]) + " secondary=" + StateInfo.secondary[id]);
            }
        }
        System.exit(0);
    }
}
