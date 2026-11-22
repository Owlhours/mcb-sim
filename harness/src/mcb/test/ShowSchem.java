package mcb.test;

import java.nio.file.Paths;
import mcb.build.StateInfo;
import mcb.build.Target;
import mcb.corpus.BuildIndex;
import mcb.env.Env;
import mcb.schem.Schem;

/** Prints a schematic layer by layer as text (one character per cell). */
public class ShowSchem {
    public static void main(String[] a) throws Exception {
        String h = a[0];
        for (String l : java.nio.file.Files.readAllLines(mcb.run.Work.corpus().resolve("unique.tsv"))) if (l.startsWith(h)) { h = l.split("\t")[0]; break; }
        Env.boot(mcb.run.Work.tmp("universe-show"));
        StateInfo.init();
        Schem s = Schem.load(BuildIndex.cacheFile(mcb.run.Work.corpus(), h));
        Target t = new Target(s);
        java.util.Map<String, Character> sym = new java.util.LinkedHashMap<>();
        String symbols = "#%@&*+=oxOX$abcdefghijklmnpqrstuvwyzABCDEFGHIJKLMNPQRSTUVWYZ0123456789";
        int from = a.length > 1 ? Integer.parseInt(a[1]) : 0, to = a.length > 2 ? Integer.parseInt(a[2]) : t.sy - 1;
        for (int y = from; y <= Math.min(to, t.sy - 1); y++) {
            System.out.println("LAYER " + y);
            for (int z = 0; z < t.sz; z++) {
                StringBuilder sb = new StringBuilder();
                for (int x = 0; x < t.sx; x++) {
                    int id = t.get(x, y, z);
                    if (id == 0) { sb.append('.'); continue; }
                    String n = StateInfo.state[id].getBlock().getDescriptionId().replace("block.minecraft.", "");
                    Character ch = sym.get(n);
                    if (ch == null) { ch = symbols.charAt(Math.min(sym.size(), symbols.length() - 1)); sym.put(n, ch); }
                    sb.append(ch);
                }
                System.out.println(sb);
            }
        }
        System.out.println("LEGEND " + sym);
        System.exit(0);
    }
}
