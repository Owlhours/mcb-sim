package mcb.run;

import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardOpenOption;
import java.util.HashSet;
import java.util.Set;
import mcb.corpus.BuildIndex;
import mcb.schem.Schem;
import mcb.schem.SchemIO;
import net.minecraft.SharedConstants;
import net.minecraft.server.Bootstrap;

/**
 * Prep <schematic>...: normalises schematic files into the corpus the simulators read:
 * {@code <MCB_WORK>/corpus/cache/<2 hex>/<hash>.mcb}, plus a row in {@code corpus/unique.tsv} (hash, sx, sy, sz,
 * non-air cells, unknown cells, format, copies, path). Prints {@code <hash> <blocks> <path>} per file, and writes the
 * hashes of this call, one per line, to {@code corpus/prep-list.txt} (a list the simulators and scripts take). The
 * hash is of the normalised content, so two files with the same build get the same hash.
 */
public final class Prep {
    public static void main(String[] a) throws Exception {
        // (the game's bootstrap routes System.out through its logger; results go to the real stdout)
        java.io.PrintStream out = System.out;
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        Path corpus = Work.corpus();
        Files.createDirectories(corpus);
        Path index = corpus.resolve("unique.tsv");
        Set<String> known = new HashSet<>();
        if (Files.exists(index)) for (String l : Files.readAllLines(index)) known.add(l.split("\t", 2)[0]);
        SchemIO io = mcb.mc.GameStates.reader();
        StringBuilder list = new StringBuilder();
        int bad = 0;
        for (String f : a) {
            Path p = Paths.get(f);
            try {
                Schem s = io.read(p);
                String h = s.contentHash();
                Path c = BuildIndex.cacheFile(corpus, h);
                Files.createDirectories(c.getParent());
                if (!Files.exists(c)) s.save(c);
                if (known.add(h)) Files.writeString(index, String.join("\t", h, "" + s.sx, "" + s.sy, "" + s.sz, "" + s.nonAir(), "" + s.unknownCells,
                    s.format, "1", p.getFileName().toString()) + "\n", StandardOpenOption.CREATE, StandardOpenOption.APPEND);
                out.println(h + "\t" + s.nonAir() + "\t" + p);
                list.append(h).append('\n');
            } catch (Exception e) {
                bad++;
                out.println("ERROR\t" + p + "\t" + e);
            }
        }
        Files.writeString(corpus.resolve("prep-list.txt"), list.toString());
        System.exit(bad == 0 ? 0 : 1);
    }
}
