package mcb.run;

import java.io.BufferedWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardOpenOption;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import mcb.corpus.BuildIndex;
import mcb.env.Env;
import mcb.schem.Schem;

/**
 * Runs the bot on a list of builds (hashes from corpus/unique.tsv, one per line) and appends one line per build to
 * a results file. Restartable: builds already in the results file are skipped. Exits after maxBuilds so that a
 * wrapper script can start a fresh server process (each build uses a new patch of the same world).
 */
public final class RunCorpus {
    public static void main(String[] args) throws Exception {
        Path list = Paths.get(args[0]), results = Paths.get(args[1]);
        int maxBuilds = args.length > 2 ? Integer.parseInt(args[2]) : 200;
        long maxWallMs = (args.length > 3 ? Long.parseLong(args[3]) : 3600) * 1000L;
        Path corpus = Paths.get("/work/corpus");
        Set<String> done = new HashSet<>();
        if (Files.exists(results)) for (String l : Files.readAllLines(results)) done.add(l.split("\t", 2)[0]);
        List<String> todo = new ArrayList<>();
        for (String l : Files.readAllLines(list)) { String h = l.trim().split("\\s+")[0]; if (!h.isEmpty() && !done.contains(h)) todo.add(h); }
        if (todo.isEmpty()) { System.out.println("nothing to do"); System.exit(3); }
        if (args.length > 4) BuildRun.wallLimitMs = Long.parseLong(args[4]) * 1000L;
        Path universe = Paths.get("/work/tmp/universe-" + ProcessHandle.current().pid());
        Env env = Env.boot(universe);
        long t0 = System.currentTimeMillis();
        int n = 0;
        try (BufferedWriter w = Files.newBufferedWriter(results, StandardOpenOption.CREATE, StandardOpenOption.APPEND)) {
            for (String h : todo) {
                if (n >= maxBuilds || System.currentTimeMillis() - t0 > maxWallMs) break;
                n++;
                String line;
                try {
                    Schem s = Schem.load(BuildIndex.cacheFile(corpus, h));
                    BuildRun.Outcome o = BuildRun.run(env, s, BuildRun.TICK_LIMIT, System.getenv("MCB_VERBOSE") != null && n > 1);
                    StringBuilder bad = new StringBuilder();
                    o.judge.badByBlock.entrySet().stream().sorted((a, b) -> b.getValue() - a.getValue()).limit(8)
                        .forEach((Map.Entry<String, Integer> e) -> bad.append(e.getKey()).append('=').append(e.getValue()).append("; "));
                    line = String.join("\t", h, o.judge.pass ? "PASS" : "FAIL", o.status, Long.toString(o.targetBlocks), Long.toString(o.ticks), Long.toString(o.placed),
                        Long.toString(o.skipped), Long.toString(o.judge.missing), Long.toString(o.judge.wrong), Long.toString(o.judge.leftover),
                        Integer.toString(o.judge.sections), Integer.toString(o.judge.failedSections), Long.toString(o.judge.exemptCells), Long.toString(o.scaffoldPlaced),
                        Long.toString(o.scaffoldLeft), Long.toString(o.trips), Long.toString(o.wallMs), o.judge.passByVolume ? "volPASS" : "volFAIL", bad.toString(), o.skipReasons, s.sx + "x" + s.sy + "x" + s.sz);
                    System.out.println(h + " " + s.sx + "x" + s.sy + "x" + s.sz + " " + o);
                } catch (Throwable e) {
                    e.printStackTrace();
                    line = h + "\tFAIL\tcrash: " + String.valueOf(e).replace('\t', ' ').replace('\n', ' ');
                    if (e instanceof OutOfMemoryError) { w.write(line); w.newLine(); w.flush(); System.exit(2); }
                }
                w.write(line);
                w.newLine();
                w.flush();
            }
        }
        try (var st = Files.walk(universe)) { st.sorted(java.util.Comparator.reverseOrder()).forEach(p -> p.toFile().delete()); } catch (Exception ignored) { }
        System.exit(n < todo.size() ? 0 : 3);
    }
}
