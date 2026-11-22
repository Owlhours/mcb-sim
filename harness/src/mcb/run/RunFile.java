package mcb.run;

import mcb.headless.BuildRun;
import java.nio.file.Path;
import java.nio.file.Paths;
import mcb.env.Env;
import mcb.schem.Schem;
import mcb.schem.SchemIO;

/**
 * RunFile <schematic> [quiet] [wall-seconds]: one build of a .litematic, .schem, .nbt or .schematic file (or a
 * normalised .mcb), headless, start to verdict. The same run as {@link RunOne}, without the corpus index.
 * Last line: {@code VERDICT PASS|FAIL <status> ticks=<n> ...}.
 */
public final class RunFile {
    public static void main(String[] a) throws Exception {
        // (the game's bootstrap routes System.out through its logger; the result lines go to the real stdout)
        java.io.PrintStream out = System.out;
        Path file = Paths.get(a[0]);
        boolean verbose = a.length < 2 || !a[1].equals("quiet");
        if (a.length > 2) BuildRun.wallLimitMs = Long.parseLong(a[2]) * 1000L;
        Env env = Env.boot(Work.tmp("universe-file-" + ProcessHandle.current().pid()));
        Schem s = file.toString().endsWith(".mcb") ? Schem.load(file) : new SchemIO().read(file);
        out.println("BUILD " + s.contentHash() + " " + file.getFileName() + " " + s.sx + "x" + s.sy + "x" + s.sz + " blocks=" + s.nonAir() + " format=" + s.format);
        BuildRun.Outcome o = BuildRun.run(env, s, BuildRun.TICK_LIMIT, verbose);
        out.println("RESULT " + o);
        out.println("BAD " + o.judge.badByBlock);
        if (verbose) for (String e : o.judge.examples) out.println("  bad: " + e);
        out.println("SKIPS " + o.skipReasons);
        out.println("NAV " + o.navStats);
        out.println("VERDICT " + (o.judge.pass ? "PASS" : "FAIL") + " " + o.status.replace(' ', '_') + " ticks=" + o.ticks
            + " gameHours=" + String.format("%.2f", o.ticks / 72000.0) + " blocks=" + o.targetBlocks + " missing=" + o.judge.missing
            + " wrong=" + o.judge.wrong + " leftover=" + o.judge.leftover + " sections=" + o.judge.sections + " failedSections=" + o.judge.failedSections
            + " wall=" + String.format("%.1fs", o.wallMs / 1000.0));
        System.exit(0);
    }
}
