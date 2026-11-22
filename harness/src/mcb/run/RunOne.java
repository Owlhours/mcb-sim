package mcb.run;

import java.nio.file.Paths;
import mcb.corpus.BuildIndex;
import mcb.env.Env;
import mcb.schem.Schem;

/** Runs one build by hash (prefix allowed if unique in corpus/unique.tsv) with full logging. */
public final class RunOne {
    public static void main(String[] a) throws Exception {
        String h = a[0];
        if (h.length() < 20) {
            for (String l : java.nio.file.Files.readAllLines(Work.corpus().resolve("unique.tsv"))) if (l.startsWith(h)) { h = l.split("\t")[0]; break; }
        }
        boolean verbose = a.length < 2 || !a[1].equals("quiet");
        if (a.length > 2) BuildRun.wallLimitMs = Long.parseLong(a[2]) * 1000L;
        Env env = Env.boot(Work.tmp("universe-one-" + ProcessHandle.current().pid()));
        Schem s = Schem.load(BuildIndex.cacheFile(Work.corpus(), h));
        System.out.println("BUILD " + h + " " + s.sx + "x" + s.sy + "x" + s.sz + " blocks=" + s.nonAir());
        BuildRun.Outcome o = BuildRun.run(env, s, BuildRun.TICK_LIMIT, verbose);
        System.out.println("RESULT " + o);
        System.out.println("BAD " + o.judge.badByBlock);
        if (verbose) for (String e : o.judge.examples) System.out.println("  bad: " + e);
        System.out.println("SKIPS " + o.skipReasons);
        System.out.println("NAV " + o.navStats);
        System.exit(0);
    }
}
