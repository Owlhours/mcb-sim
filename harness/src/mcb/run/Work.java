package mcb.run;

import java.nio.file.Path;
import java.nio.file.Paths;

/**
 * The working directory for data that is not in git: the schematic cache ({@code corpus/}), the generated tables
 * ({@code reference/}) and scratch worlds ({@code tmp/}). {@code MCB_WORK}, default {@code work} under the current
 * directory (harness/run.sh runs from the repository root).
 */
public final class Work {
    private Work() { }
    public static Path root() { return Paths.get(System.getenv().getOrDefault("MCB_WORK", "work")).toAbsolutePath(); }
    public static Path corpus() { return root().resolve("corpus"); }
    public static Path tmp(String name) { return root().resolve("tmp").resolve(name); }
}
