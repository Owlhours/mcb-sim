package mcb.build;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import mcb.env.Env;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.ChestBlock;
import net.minecraft.world.level.block.entity.ChestBlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.ChestType;

/**
 * A build site (administrator side): where the target goes in the world and the supply chests next to it.
 *
 * Layout, in world coordinates: the target's cell (0,0,0) is at (ox, oy, oz) with oy the first air layer above
 * the flat ground. Supply chests stand north of the build (smaller z): rows of double chests stacked three high,
 * each row facing south with a two-block aisle in front of it. The chests hold every block item the target
 * needs (plus a small reserve) and the temporary scaffolding blocks.
 */
public final class Site {
    public static final int ROW_GAP = 5;      // distance from the build to the first chest row
    public static final int ROW_PITCH = 3;
    public static final int STACK_HEIGHT = 3;

    public final Env env;
    public final Target target;
    public final int ox, oy, oz;
    public final Item scaffoldItem;
    public final int scaffoldState;
    /** One entry per chest slot that holds something. */
    public final List<Stock> stock = new ArrayList<>();
    public int depotRows, depotCols;

    public static final class Stock {
        public Item item;
        public int count;
        public final int chestX, chestY, chestZ; // the west block of the double chest (click either half)
        public final int slot;                   // 0..53 in the chest screen
        public Stock(Item item, int count, int x, int y, int z, int slot) { this.item = item; this.count = count; chestX = x; chestY = y; chestZ = z; this.slot = slot; }
    }

    public Site(Env env, Target target, int ox, int oz) {
        StateInfo.init();
        this.env = env;
        this.target = target;
        this.ox = ox;
        this.oz = oz;
        int gy = env.level.getMinY();
        BlockPos.MutableBlockPos p = new BlockPos.MutableBlockPos(ox, gy, oz);
        while (!env.level.getBlockState(p).isAir()) p.setY(++gy);
        this.oy = gy;
        // scaffold material: a quick-to-break full cube that the target itself does not use
        boolean[] used = new boolean[StateInfo.N];
        for (char c : target.ids) used[c] = true;
        Block[] candidates = {Blocks.MOSS_BLOCK, Blocks.PALE_MOSS_BLOCK, Blocks.DRIED_KELP_BLOCK, Blocks.HAY_BLOCK, Blocks.DIRT, Blocks.COBBLESTONE};
        Block chosen = candidates[0];
        for (Block b : candidates) {
            boolean inUse = false;
            for (BlockState s : b.getStateDefinition().getPossibleStates()) if (used[Block.getId(s)]) inUse = true;
            if (!inUse) { chosen = b; break; }
        }
        this.scaffoldItem = chosen.asItem();
        this.scaffoldState = Block.getId(chosen.defaultBlockState());
    }

    public boolean fitsWorld() { return oy + target.sy + 4 <= env.level.getMaxY(); }

    /** Item requirements of the target: item -> count. */
    public Map<Item, Integer> requirements() {
        Map<Item, Integer> need = new LinkedHashMap<>();
        int[] perState = new int[StateInfo.N];
        for (char c : target.ids) perState[c]++;
        for (int id = 1; id < StateInfo.N; id++) {
            if (perState[id] == 0 || StateInfo.itemCount[id] == 0) continue;
            Item it = StateInfo.item[id];
            if (it == Items.AIR) continue;
            need.merge(it, perState[id] * StateInfo.itemCount[id], Integer::sum);
            Item extra = StateInfo.contentItem(id);
            if (extra != null) need.merge(extra, perState[id], Integer::sum);
        }
        return need;
    }

    /** Places and fills the supply chests. */
    public void setUp(int scaffoldCount) {
        Map<Item, Integer> need = requirements();
        List<ItemStack> stacks = new ArrayList<>();
        addStacks(stacks, scaffoldItem, scaffoldCount);
        for (Map.Entry<Item, Integer> e : need.entrySet()) {
            int n = e.getValue();
            addStacks(stacks, e.getKey(), n + Math.max(4, n / 20));
        }
        int perStack = 54 * STACK_HEIGHT;
        int chestStacks = Math.max(1, (stacks.size() + perStack - 1) / perStack);
        depotCols = Math.max(3, Math.min(24, (target.sx + 1) / 2));
        depotRows = (chestStacks + depotCols - 1) / depotCols;
        int si = 0;
        for (int k = 0; k < chestStacks; k++) {
            int row = k / depotCols, col = k % depotCols;
            int cx = ox + col * 2, cz = oz - ROW_GAP - row * ROW_PITCH;
            for (int h = 0; h < STACK_HEIGHT && si < stacks.size(); h++) {
                int cy = oy + h;
                BlockPos west = new BlockPos(cx, cy, cz), east = west.east();
                BlockState base = Blocks.CHEST.defaultBlockState().setValue(ChestBlock.FACING, Direction.SOUTH);
                env.level.setBlock(west, base.setValue(ChestBlock.TYPE, ChestType.RIGHT), 2);
                env.level.setBlock(east, base.setValue(ChestBlock.TYPE, ChestType.LEFT), 2);
                ChestBlockEntity bw = (ChestBlockEntity) env.level.getBlockEntity(west), be = (ChestBlockEntity) env.level.getBlockEntity(east);
                for (int s = 0; s < 54 && si < stacks.size(); s++, si++) {
                    ItemStack st = stacks.get(si);
                    (s < 27 ? bw : be).setItem(s % 27, st.copy());
                    stock.add(new Stock(st.getItem(), st.getCount(), cx, cy, cz, s));
                }
            }
        }
    }

    private static void addStacks(List<ItemStack> out, Item item, int count) {
        int max = new ItemStack(item).getMaxStackSize();
        while (count > 0) {
            int n = Math.min(max, count);
            out.add(new ItemStack(item, n));
            count -= n;
        }
    }

    /** The world's current content of the target box (for judging). */
    public char[] snapshot() {
        char[] out = new char[target.ids.length];
        BlockPos.MutableBlockPos p = new BlockPos.MutableBlockPos();
        int i = 0;
        for (int y = 0; y < target.sy; y++) for (int z = 0; z < target.sz; z++) for (int x = 0; x < target.sx; x++, i++) {
            out[i] = (char) Block.getId(env.level.getBlockState(p.set(ox + x, oy + y, oz + z)));
        }
        return out;
    }

    /** Test helper: writes the target straight into the world (as a creative-mode paste would). */
    public void pasteTarget(int flags) {
        BlockPos.MutableBlockPos p = new BlockPos.MutableBlockPos();
        int i = 0;
        for (int y = 0; y < target.sy; y++) for (int z = 0; z < target.sz; z++) for (int x = 0; x < target.sx; x++, i++) {
            int id = target.ids[i];
            if (id != 0) env.level.setBlock(p.set(ox + x, oy + y, oz + z), StateInfo.state[id], flags);
        }
    }
}
