package mcb.run;

import java.io.BufferedWriter;
import java.io.PrintWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import mcb.build.StateInfo;
import mcb.env.Env;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.Property;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.shapes.Shapes;
import net.minecraft.world.phys.shapes.VoxelShape;

/**
 * DumpStates <out.tsv>: one row per block state (index = global state id), the per-state table the Rust simulators
 * read ({@code reference/mc/states.tsv}). Columns: id, block, props, the flags and small tables of {@link StateInfo},
 * then friction, speed and jump factors, destroy time, requiresTool, light, and the collision and outline boxes
 * ({@code minX,minY,minZ,maxX,maxY,maxZ;...}, block-local).
 */
public final class DumpStates {
    public static void main(String[] a) throws Exception {
        Path out = Paths.get(a.length > 0 ? a[0] : "work/reference/mc/states.tsv");
        Files.createDirectories(out.toAbsolutePath().getParent());
        Env.boot(Work.tmp("universe-dump-" + ProcessHandle.current().pid()));
        StateInfo.init();
        EmptyBlockGetter g = EmptyBlockGetter.INSTANCE;
        try (PrintWriter w = new PrintWriter(new BufferedWriter(Files.newBufferedWriter(out)))) {
            w.print("id\tblock\tprops\tair\tfluid\tgravity\treplaceable\tfullCube\tnoCollision\tscaffolding\twalkThrough\tsturdyTop\tdoorEdges"
                + "\thandDoor\tclosedGate\thazard\trung\trungFacing\tleaves\tclimbable\tinteractive\tsecondary\tlate\ttechnical\tdewater\tcanon\tloose"
                + "\titem\titemCount\tcolTop\tcolBottom\tanyTop\tanyBottom\tfriction\tspeedFactor\tjumpFactor\tdestroyTime\trequiresTool\tlight"
                + "\tcollisionBoxes\toutlineBoxes\n");
            for (int i = 0; i < StateInfo.N; i++) {
                BlockState s = Block.stateById(i);
                Block b = s.getBlock();
                StringBuilder r = new StringBuilder();
                r.append(i).append('\t').append(BuiltInRegistries.BLOCK.getKey(b).getPath()).append('\t');
                boolean first = true;
                for (Property<?> p : s.getProperties()) {
                    if (!first) r.append(',');
                    first = false;
                    r.append(p.getName()).append('=').append(value(s, p));
                }
                f(r, StateInfo.air[i]); f(r, StateInfo.fluid[i]); f(r, StateInfo.gravity[i]); f(r, StateInfo.replaceable[i]);
                f(r, StateInfo.fullCube[i]); f(r, StateInfo.noCollision[i]); f(r, StateInfo.scaffolding[i]); f(r, StateInfo.walkThrough[i]);
                f(r, StateInfo.sturdyTop[i]); r.append('\t').append(StateInfo.doorEdges[i]); f(r, StateInfo.handDoor[i]);
                f(r, StateInfo.closedGate[i]);
                // the hazard column as the Java bot defined it when the table was first made (commit f6b21f4): it still
                // counted magma, berry bushes and powder snow; the simulators were validated against that file
                f(r, StateInfo.hazard[i] || b == net.minecraft.world.level.block.Blocks.MAGMA_BLOCK
                    || b instanceof net.minecraft.world.level.block.SweetBerryBushBlock || b == net.minecraft.world.level.block.Blocks.POWDER_SNOW); r.append('\t').append(StateInfo.rung[i]);
                r.append('\t').append(StateInfo.rungFacing[i]); f(r, StateInfo.leaves[i]); f(r, StateInfo.climbable[i]);
                f(r, StateInfo.interactive[i]); f(r, StateInfo.secondary[i]); f(r, StateInfo.late[i]); f(r, StateInfo.technical[i]);
                r.append('\t').append((int) StateInfo.dewater[i]).append('\t').append((int) StateInfo.canon[i]).append('\t').append((int) StateInfo.loose[i]);
                r.append('\t').append(BuiltInRegistries.ITEM.getKey(StateInfo.item[i]).getPath()).append('\t').append(StateInfo.itemCount[i]);
                r.append('\t').append(StateInfo.colTop[i]).append('\t').append(StateInfo.colBottom[i]);
                r.append('\t').append(StateInfo.anyTop[i]).append('\t').append(StateInfo.anyBottom[i]);
                r.append('\t').append(b.getFriction()).append('\t').append(b.getSpeedFactor()).append('\t').append(b.getJumpFactor());
                float dt; try { dt = s.getDestroySpeed(g, BlockPos.ZERO); } catch (RuntimeException e) { dt = -1; }
                r.append('\t').append(dt);
                f(r, s.requiresCorrectToolForDrops());
                r.append('\t').append(s.getLightEmission());
                VoxelShape col, out2;
                try { col = s.getCollisionShape(g, BlockPos.ZERO); } catch (RuntimeException e) { col = Shapes.block(); }
                try { out2 = s.getShape(g, BlockPos.ZERO); } catch (RuntimeException e) { out2 = Shapes.block(); }
                r.append('\t').append(boxes(col)).append('\t').append(boxes(out2));
                w.print(r.append('\n'));
            }
        }
        System.out.println("states: " + StateInfo.N + " rows in " + out);
        System.exit(0);
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static String value(BlockState s, Property p) { return p.getName(s.getValue(p)); }

    private static void f(StringBuilder r, boolean v) { r.append('\t').append(v ? '1' : '0'); }

    private static String boxes(VoxelShape sh) {
        StringBuilder r = new StringBuilder();
        for (AABB x : sh.toAabbs()) {
            if (r.length() > 0) r.append(';');
            r.append(x.minX).append(',').append(x.minY).append(',').append(x.minZ).append(',').append(x.maxX).append(',').append(x.maxY).append(',').append(x.maxZ);
        }
        return r.toString();
    }
}
