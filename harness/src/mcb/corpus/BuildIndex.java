package mcb.corpus;

import java.io.BufferedWriter;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardOpenOption;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import mcb.schem.Schem;
import mcb.schem.SchemIO;
import net.minecraft.SharedConstants;
import net.minecraft.server.Bootstrap;

/**
 * Pass 1 over the collection: read every file in corpus/files.txt, normalise it and store one cached copy per
 * distinct content under corpus/cache/, and append one line per file to corpus/index.tsv:
 *   path, status(ok|err), hash, sx, sy, sz, nonAir, unknownCells, format, dataVersion, message
 * Restartable: files already in the index are skipped.
 */
public final class BuildIndex {
    public static Path cacheFile(Path corpus, String hash) { return corpus.resolve("cache").resolve(hash.substring(0, 2)).resolve(hash + ".mcb"); }

    public static void main(String[] args) throws Exception {
        Path corpus = Paths.get(args.length > 0 ? args[0] : "/work/corpus");
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        List<String> files = Files.readAllLines(corpus.resolve("files.txt"));
        Path index = corpus.resolve("index.tsv");
        Set<String> done = new HashSet<>();
        Set<String> hashes = new HashSet<>();
        if (Files.exists(index)) for (String l : Files.readAllLines(index)) {
            String[] f = l.split("\t", -1);
            done.add(f[0]);
            if (f.length > 2 && f[1].equals("ok")) hashes.add(f[2]);
        }
        SchemIO io = new SchemIO();
        long t0 = System.currentTimeMillis();
        int n = 0, ok = 0, err = 0;
        try (BufferedWriter w = Files.newBufferedWriter(index, StandardOpenOption.CREATE, StandardOpenOption.APPEND)) {
            for (String f : files) {
                if (f.isBlank() || done.contains(f)) continue;
                n++;
                String line;
                try {
                    Path p = Paths.get(f);
                    if (Files.size(p) > 300_000_000L) throw new IOException("file too large");
                    Schem s = io.read(p);
                    String h = s.contentHash();
                    if (hashes.add(h)) s.save(cacheFile(corpus, h));
                    line = f + "\tok\t" + h + "\t" + s.sx + "\t" + s.sy + "\t" + s.sz + "\t" + s.nonAir() + "\t" + s.unknownCells + "\t" + s.format + "\t" + s.dataVersion + "\t";
                    ok++;
                } catch (Throwable e) {
                    String m = String.valueOf(e.getMessage());
                    line = f + "\terr\t\t0\t0\t0\t0\t0\t\t0\t" + e.getClass().getSimpleName() + ": " + m.replace('\t', ' ').replace('\n', ' ');
                    if (line.length() > 600) line = line.substring(0, 600);
                    err++;
                }
                w.write(line);
                w.newLine();
                if (n % 200 == 0) {
                    w.flush();
                    System.out.printf("%d files (%d ok, %d err, %d unique) in %d s%n", n, ok, err, hashes.size(), (System.currentTimeMillis() - t0) / 1000);
                }
            }
        }
        System.out.printf("done: %d files (%d ok, %d err), %d unique builds, %d s%n", n, ok, err, hashes.size(), (System.currentTimeMillis() - t0) / 1000);
        System.exit(0);
    }
}
