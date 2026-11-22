package mcb.run;

import java.io.*;
import java.nio.file.Paths;
import java.util.zip.GZIPOutputStream;
import mcb.env.Env;
import mcb.env.Oracle;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.item.BlockItem;
import net.minecraft.world.item.Item;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.SupportType;
import net.minecraft.world.level.block.state.BlockState;

/**
 * DumpRef <outdir>: reference data from the Java bot for the Rust simulator: {@code states-extra.tsv} (more per-state
 * facts, read by the simulator when present), {@code floats.bin.gz} and {@code placement-tables.bin.gz} with its index
 * (read only by the {@code ref_check} test). The placement tables take a few minutes.
 */
public final class DumpRef {
    public static void main(String[] a) throws Exception {
        File out = new File(a[0]); out.mkdirs();
        Env env = Env.boot(Work.tmp("universe-dump-" + ProcessHandle.current().pid()));
        Oracle oracle = new Oracle(env);

        // 3. extra per-state columns
        int n = Block.BLOCK_STATE_REGISTRY.size();
        try (PrintWriter w = new PrintWriter(new BufferedWriter(new FileWriter(new File(out, "states-extra.tsv"))))) {
            StringBuilder h = new StringBuilder("id\tblock\tisRedstoneConductor\tisSolid\thasBlockEntity\tpushReaction\tisSuffocating\tinstrument\tdefaultStateId");
            String[] st = {"FULL", "CENTER", "RIGID"};
            for (Direction d : Direction.values()) for (String s : st) h.append("\tsturdy_").append(d.getName()).append('_').append(s);
            w.println(h);
            SupportType[] sts = {SupportType.FULL, SupportType.CENTER, SupportType.RIGID};
            for (int id = 0; id < n; id++) {
                BlockState s = Block.stateById(id);
                StringBuilder r = new StringBuilder().append(id).append('\t').append(BuiltInRegistries.BLOCK.getKey(s.getBlock()).getPath());
                r.append('\t').append(b(() -> s.isRedstoneConductor(EmptyBlockGetter.INSTANCE, BlockPos.ZERO)));
                r.append('\t').append(b(() -> s.isSolid()));
                r.append('\t').append(b(() -> s.hasBlockEntity()));
                String pr; try { pr = s.getPistonPushReaction().name(); } catch (RuntimeException e) { pr = "ERR"; }
                r.append('\t').append(pr);
                r.append('\t').append(b(() -> s.isSuffocating(EmptyBlockGetter.INSTANCE, BlockPos.ZERO)));
                String in; try { in = s.instrument().name(); } catch (RuntimeException e) { in = "ERR"; }
                r.append('\t').append(in);
                r.append('\t').append(Block.getId(s.getBlock().defaultBlockState()));
                for (Direction d : Direction.values()) for (SupportType t : sts)
                    r.append('\t').append(b(() -> s.isFaceSturdy(EmptyBlockGetter.INSTANCE, BlockPos.ZERO, d, t)));
                w.println(r);
            }
        }
        System.out.println("states-extra done: " + n);

        // 2b. floats(state)
        byte[] fl = new byte[n];
        for (int id = 0; id < n; id++) fl[id] = (byte) (oracle.floats(id) ? 1 : 0);
        try (OutputStream o = new GZIPOutputStream(new FileOutputStream(new File(out, "floats.bin.gz")))) { o.write(fl); }
        System.out.println("floats done");

        // 2a. placement tables for every item (BlockItem or not; non-block items are all zeros)
        try (DataOutputStream o = new DataOutputStream(new BufferedOutputStream(new GZIPOutputStream(new FileOutputStream(new File(out, "placement-tables.bin.gz")))));
             PrintWriter idx = new PrintWriter(new FileWriter(new File(out, "placement-tables.index.tsv")))) {
            idx.println("index\titem\tis_block_item\tnonzero_entries");
            int k = 0;
            for (Item item : BuiltInRegistries.ITEM) {
                char[] t = oracle.table(item);
                int nz = 0; for (char c : t) if (c != 0) nz++;
                idx.println(k + "\t" + BuiltInRegistries.ITEM.getKey(item).getPath() + "\t" + (item instanceof BlockItem ? 1 : 0) + "\t" + nz);
                for (char c : t) o.writeChar(c);   // big-endian u16
                k++;
                if (k % 100 == 0) System.out.println("tables " + k);
            }
            System.out.println("tables done: " + k + " items x " + Oracle.TABLE_SIZE + " entries");
        }
        System.exit(0);
    }
    interface BS { boolean get(); }
    static String b(BS f) { try { return f.get() ? "1" : "0"; } catch (RuntimeException e) { return "E"; } }
}
