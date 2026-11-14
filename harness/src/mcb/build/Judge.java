package mcb.build;

/**
 * Scores a finished build against its target, section by section (16x16x16 cells from the schematic's origin).
 * In each section: bad = wrong + missing + left-behind (a block where the target has none). Decorative blocks
 * (no collision box) are exempt: their cells are not judged. Leaves count as right if the same kind of leaves is
 * there, whatever their decay state. A section passes with bad <= 3 % of its judged target cells (a section with
 * no target cells passes only if nothing was left in it); the build passes when every section passes.
 */
public final class Judge {
    public static final double LIMIT = 0.03;

    public static final class Result {
        public int sections, failedSections;
        public long targetCells, wrong, missing, leftover, exemptCells;
        public long strictMismatch;            // judged cells whose exact state (ignoring waterlogging) differs
        public double worstRatio;              // worst bad/target over sections
        public boolean pass;
        public int failedSectionsByVolume;     // alternative reading: bad <= 3 % of the section's volume
        public boolean passByVolume;
        /** The same build under the wider reading of the exemptions (see {@link StateInfo#decorativeWide}); game-master blocks not counted. */
        public boolean passWide;
        public int failedSectionsWide;
        public double worstRatioWide;
        public String firstBad = "";
        public java.util.List<String> examples = new java.util.ArrayList<>();
        public java.util.TreeMap<String, Integer> badByBlock = new java.util.TreeMap<>();

        @Override public String toString() {
            return String.format("%s sections=%d failed=%d target=%d wrong=%d missing=%d leftover=%d exempt=%d strictDiff=%d worst=%.3f byVolume=%s wide=%s(failed=%d worst=%.3f)%s",
                pass ? "PASS" : "FAIL", sections, failedSections, targetCells, wrong, missing, leftover, exemptCells, strictMismatch, worstRatio,
                passByVolume ? "PASS" : "FAIL", passWide ? "PASS" : "FAIL", failedSectionsWide, worstRatioWide, firstBad.isEmpty() ? "" : " first=" + firstBad);
        }
    }

    public static boolean cellOk(int target, int world) {
        if (target == world) return true;
        return StateInfo.canon[target] == StateInfo.canon[world];
    }

    static final boolean OLD_JUDGE = System.getenv("MCB_OLD_JUDGE") != null;

    /** @param world the built region's states, same layout as target.ids */
    public static Result judge(Target t, char[] world) {
        StateInfo.init();
        Result r = new Result();
        int nx = (t.sx + 15) >> 4, ny = (t.sy + 15) >> 4, nz = (t.sz + 15) >> 4;
        for (int cy = 0; cy < ny; cy++) for (int cz = 0; cz < nz; cz++) for (int cx = 0; cx < nx; cx++) {
            int tgt = 0, bad = 0, vol = 0, tgtW = 0, badW = 0;
            int x1 = Math.min(t.sx, (cx + 1) << 4), y1 = Math.min(t.sy, (cy + 1) << 4), z1 = Math.min(t.sz, (cz + 1) << 4);
            for (int y = cy << 4; y < y1; y++) for (int z = cz << 4; z < z1; z++) {
                int i = (y * t.sz + z) * t.sx + (cx << 4);
                for (int x = cx << 4; x < x1; x++, i++) {
                    vol++;
                    int a = t.ids[i], w = world[i];
                    if (StateInfo.air[w]) w = 0;
                    // the wider reading, counted alongside
                    if (a == 0) { if (w != 0 && !StateInfo.fluid[w] && !StateInfo.technical[w] && !StateInfo.secondary[w]) badW++; }
                    else if (!StateInfo.gameMaster(a)) {
                        // ("3 % of all blocks in a section": exempt decoration cannot be bad, but it is among the blocks)
                        tgtW++;
                        if (!StateInfo.decorativeWide(a) && (w == 0 || !cellOk(a, w))) badW++;
                    }
                    // (exempt: decoration, and - from 2026-11-14, the owner's decision - the blocks only an operator in
                    // creative mode can place: command blocks, structure block, jigsaw, test blocks. Their cells are
                    // neither missing nor part of what the 3 % is taken of. MCB_OLD_JUDGE=1 counts the latter as before.)
                    if (a != 0 && (StateInfo.decorative(a) || (!OLD_JUDGE && StateInfo.gameMaster(a)))) { r.exemptCells++; continue; }
                    if (a == 0) {
                        // (a piston's head where the target shows the piston drawn in is part of that piston, not a block left behind)
                        // (nor the upper half of a door or the head of a bed where the target only has the other part)
                        if (w != 0 && !StateInfo.fluid[w] && !StateInfo.technical[w] && !StateInfo.secondary[w]) { bad++; r.leftover++; note(r, "leftover " + StateInfo.name(w), x, y, z); }
                        continue;
                    }
                    tgt++;
                    if (w == 0) { bad++; r.missing++; note(r, "missing " + StateInfo.name(a), x, y, z); }
                    else if (!cellOk(a, w)) { bad++; r.wrong++; note(r, "wrong " + StateInfo.name(a) + " <- " + StateInfo.name(w), x, y, z); }
                    else if (StateInfo.dewater[w] != a) r.strictMismatch++;
                }
            }
            r.sections++;
            r.targetCells += tgt;
            double ratio = tgt == 0 ? (bad == 0 ? 0 : 1) : (double) bad / tgt;
            if (ratio > r.worstRatio) r.worstRatio = ratio;
            if (ratio > LIMIT) r.failedSections++;
            double ratioW = tgtW == 0 ? (badW == 0 ? 0 : 1) : (double) badW / tgtW;
            if (ratioW > r.worstRatioWide) r.worstRatioWide = ratioW;
            if (ratioW > LIMIT) r.failedSectionsWide++;
            if ((double) bad / vol > LIMIT) r.failedSectionsByVolume++;
        }
        r.pass = r.failedSections == 0;
        r.passByVolume = r.failedSectionsByVolume == 0;
        r.passWide = r.failedSectionsWide == 0;
        return r;
    }

    private static void note(Result r, String what, int x, int y, int z) {
        if (r.firstBad.isEmpty()) r.firstBad = what + " @" + x + "," + y + "," + z;
        if (r.examples.size() < 400) r.examples.add(what + " @" + x + "," + y + "," + z);
        String k = what.replaceAll("\\[.*?\\]", "");
        if (r.badByBlock.size() < 400 || r.badByBlock.containsKey(k)) r.badByBlock.merge(k, 1, Integer::sum);
    }
}
