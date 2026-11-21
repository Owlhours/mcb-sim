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
        /** (for reports, e.g. the mod's /mcb judge; no verdict reads it) every section: {cx, cy, cz, judged cells, bad cells} */
        public java.util.List<int[]> sectionList = new java.util.ArrayList<>();

        @Override public String toString() {
            return String.format("%s sections=%d failed=%d target=%d wrong=%d missing=%d leftover=%d exempt=%d strictDiff=%d worst=%.3f byVolume=%s wide=%s(failed=%d worst=%.3f)%s",
                pass ? "PASS" : "FAIL", sections, failedSections, targetCells, wrong, missing, leftover, exemptCells, strictMismatch, worstRatio,
                passByVolume ? "PASS" : "FAIL", passWide ? "PASS" : "FAIL", failedSectionsWide, worstRatioWide, firstBad.isEmpty() ? "" : " first=" + firstBad);
        }
    }

    /** The judge's "stands right" (the sim's build::right_cell): cellOk, or - the owner's rule of 2026-11-20 - a bamboo
     *  shoot where the target has bamboo (the bamboo item placed on the ground gives bamboo_sapling, which grows into it).
     *  The judge only: the bot's own picture keeps cellOk. */
    public static boolean rightCell(int target, int world) {
        if (cellOk(target, world)) return true;
        return StateInfo.state[target].getBlock() == net.minecraft.world.level.block.Blocks.BAMBOO
            && StateInfo.state[world].getBlock() == net.minecraft.world.level.block.Blocks.BAMBOO_SAPLING;
    }

    public static boolean cellOk(int target, int world) {
        if (target == world) return true;
        return StateInfo.canon[target] == StateInfo.canon[world];
    }

    static final boolean OLD_JUDGE = System.getenv("MCB_OLD_JUDGE") != null;

    /**
     * The impossible-cell rule (the owner's decision, 2026-11-20; sim `build::impossible_cells`): ON unless
     * MCB_IMPOSSIBLE=0. Cells whose target cannot exist in survival count as standing right whatever stands there
     * (in the section's count, never bad; second form of 2026-11-20).
     */
    static final boolean IMPOSSIBLE = !"0".equals(System.getenv("MCB_IMPOSSIBLE"));

    /** The ground the judged target stands on (the sim's GROUND_TOP: grass, two dirt, bedrock, air below). */
    static final int GROUND_TOP = -61;

    /** Methods of the view that fell through to the real level (for checking the port; should stay empty). */
    public static final java.util.Set<String> viewFallThrough = java.util.concurrent.ConcurrentHashMap.newKeySet();

    /** Blocks whose item cannot be had in survival and which nothing in survival makes in place (sim `not_in_survival`). */
    static boolean notInSurvival(String name) {
        switch (name) {
            case "chorus_plant": case "budding_amethyst": case "reinforced_deepslate": case "spawner": case "trial_spawner": case "vault":
            case "frogspawn": case "suspicious_sand": case "suspicious_gravel": case "petrified_oak_slab": case "end_portal_frame":
            case "bedrock": case "barrier": case "light": case "structure_void": return true;
            default: return name.startsWith("infested_");
        }
    }

    static String blockName(int id) {
        return net.minecraft.core.registries.BuiltInRegistries.BLOCK.getKey(StateInfo.state[id].getBlock()).getPath();
    }

    /**
     * The cells whose target state cannot exist in survival, by class ("no item", "no item in survival", "live coral",
     * "cannot stay"), in the target as judged standing on the ground; "cannot stay" by the game's own canSurvive (and
     * gravity blocks over nothing), applied until nothing changes. Decoration and game-master blocks are left out of
     * the result (they are exempt anyway). Port of the sim's `build::impossible_cells`.
     * @param base a real level, only for methods of the view that are not about blocks, fluids or light
     */
    public static String[] impossibleCells(Target t, net.minecraft.world.level.LevelReader base) {
        StateInfo.init();
        final int sx = t.sx, sy = t.sy, sz = t.sz, oy = GROUND_TOP + 1;
        final char[] w = t.ids.clone();
        String[] cls = new String[t.ids.length];
        for (int i = 0; i < t.ids.length; i++) {
            int a = t.ids[i];
            if (a == 0) continue;
            String name = blockName(a);
            if (StateInfo.item[a] == net.minecraft.world.item.Items.AIR) cls[i] = "no item";
            else if (notInSurvival(name)) cls[i] = "no item in survival";
            else if (name.contains("coral") && !name.startsWith("dead_")) cls[i] = "live coral";
        }
        // (the owner's rule of 2026-11-20, second part: the operator's blocks are exempt and no player puts them up, so they
        // come out of the test world as well, and what hangs from them falls with them; the final filter keeps them out)
        for (int i = 0; i < t.ids.length; i++) { int a = t.ids[i]; if (a != 0 && cls[i] == null && StateInfo.gameMaster(a)) cls[i] = "game master"; }
        for (int i = 0; i < cls.length; i++) if (cls[i] != null) w[i] = 0;
        final net.minecraft.world.level.block.state.BlockState air = net.minecraft.world.level.block.Blocks.AIR.defaultBlockState(),
            grass = net.minecraft.world.level.block.Blocks.GRASS_BLOCK.defaultBlockState(),
            dirt = net.minecraft.world.level.block.Blocks.DIRT.defaultBlockState(),
            bedrock = net.minecraft.world.level.block.Blocks.BEDROCK.defaultBlockState();
        java.util.function.Function<net.minecraft.core.BlockPos, net.minecraft.world.level.block.state.BlockState> at = q -> {
            int x = q.getX(), y = q.getY() - oy, z = q.getZ();
            if (x >= 0 && y >= 0 && z >= 0 && x < sx && y < sy && z < sz) return StateInfo.state[w[(y * sz + z) * sx + x]];
            int qy = q.getY();
            if (qy > GROUND_TOP) return air;
            if (qy == GROUND_TOP) return grass;
            if (qy >= GROUND_TOP - 2) return dirt;
            if (qy == GROUND_TOP - 3) return bedrock;
            return air;
        };
        java.lang.reflect.InvocationHandler h = (proxy, m, args) -> {
            String n = m.getName();
            if (args != null && args.length >= 1 && args[0] instanceof net.minecraft.core.BlockPos q) {
                if (n.equals("getBlockState") && args.length == 1) return at.apply(q);
                if (n.equals("getFluidState") && args.length == 1) return net.minecraft.world.level.material.Fluids.EMPTY.defaultFluidState();
                if (n.equals("getRawBrightness") || n.equals("getMaxLocalRawBrightness")) return 15;
                if (n.equals("getBlockEntity") && args.length == 1) return null;
            }
            if (n.equals("getBrightness") && args != null && args.length == 2) return 15;
            if (m.isDefault()) return java.lang.reflect.InvocationHandler.invokeDefault(proxy, m, args);
            viewFallThrough.add(n);
            if (base == null) throw new UnsupportedOperationException(n);
            try { return m.invoke(base, args); } catch (java.lang.reflect.InvocationTargetException e) { throw e.getCause(); }
        };
        net.minecraft.world.level.LevelReader view = (net.minecraft.world.level.LevelReader) java.lang.reflect.Proxy.newProxyInstance(
            net.minecraft.world.level.LevelReader.class.getClassLoader(), new Class<?>[] {net.minecraft.world.level.LevelReader.class}, h);
        net.minecraft.core.BlockPos.MutableBlockPos q = new net.minecraft.core.BlockPos.MutableBlockPos();
        while (true) {
            IntArrayListLite gone = new IntArrayListLite();
            for (int i = 0; i < t.ids.length; i++) {
                int a = t.ids[i];
                if (a == 0 || cls[i] != null) continue;
                int x = i % sx, y = i / (sx * sz), z = (i / sx) % sz;
                q.set(x, oy + y, z);
                boolean falls = false;
                if (StateInfo.gravity[a]) {
                    int b = net.minecraft.world.level.block.Block.getId(at.apply(new net.minecraft.core.BlockPos(x, oy + y - 1, z)));
                    falls = b == 0 || StateInfo.air[b] || StateInfo.replaceable[b];
                }
                boolean ok;
                if (falls) ok = false;
                else { try { ok = StateInfo.state[a].canSurvive(view, q.immutable()); } catch (RuntimeException e) { ok = true; } }
                if (!ok) gone.add(i);
            }
            if (gone.n != 0) { for (int k = 0; k < gone.n; k++) { cls[gone.a[k]] = "cannot stay"; w[gone.a[k]] = 0; } continue; }
            // A chain stays anywhere once placed, but is placed only against a block along its axis: it is possible when,
            // along its axis, it reaches - through chains - a cell that stands (or the ground), or when it hangs over
            // plain air (a temporary block serves). Chains whose only support is exempt cells are impossible.
            int n = t.ids.length, plane = sx * sz;
            boolean[] anchored = new boolean[n];
            IntArrayListLite stack = new IntArrayListLite();
            for (int i = 0; i < n; i++) {
                if (!isChain(t, cls, i)) continue;
                int ax = chainAxis(t.ids[i]), y = i / plane;
                boolean ok = ax == 1 && y == 0;   // (the ground under the build holds an upright chain at the bottom layer)
                for (int qi : chainNbrs(i, ax, sx, sy, sz)) {
                    if (qi < 0) continue;
                    int b = t.ids[qi];
                    if (b != 0 && cls[qi] == null && !isChainBlock(b) && !StateInfo.air[b]) ok = true;
                }
                if (ok) { anchored[i] = true; stack.add(i); }
            }
            while (stack.n > 0) {
                int i = stack.a[--stack.n];
                for (int ax = 0; ax < 3; ax++) for (int qi : chainNbrs(i, ax, sx, sy, sz))
                    if (qi >= 0 && !anchored[qi] && isChain(t, cls, qi) && chainAxis(t.ids[qi]) == ax) { anchored[qi] = true; stack.add(qi); }
            }
            boolean[] doomed = new boolean[n];
            for (int i = 0; i < n; i++) {
                if (!isChain(t, cls, i) || anchored[i]) continue;
                for (int qi : chainNbrs(i, chainAxis(t.ids[i]), sx, sy, sz))
                    if (qi >= 0 && t.ids[qi] != 0 && cls[qi] != null) { doomed[i] = true; stack.add(i); break; }
            }
            while (stack.n > 0) {
                int i = stack.a[--stack.n];
                for (int ax = 0; ax < 3; ax++) for (int qi : chainNbrs(i, ax, sx, sy, sz))
                    if (qi >= 0 && !doomed[qi] && !anchored[qi] && isChain(t, cls, qi) && chainAxis(t.ids[qi]) == ax) { doomed[qi] = true; stack.add(qi); }
            }
            boolean any = false;
            for (int i = 0; i < n; i++) if (doomed[i]) { cls[i] = "hangs from nothing"; w[i] = 0; any = true; }
            if (!any) break;
        }
        for (int i = 0; i < cls.length; i++) {
            int a = t.ids[i];
            if (cls[i] != null && (StateInfo.decorative(a) || StateInfo.gameMaster(a))) cls[i] = null;
        }
        return cls;
    }

    /** The sim's Kind::ChainBlock: the block class is exactly ChainBlock (the kinds table is by class name). */
    static boolean isChainBlock(int s) { return s != 0 && StateInfo.state[s].getBlock().getClass() == net.minecraft.world.level.block.ChainBlock.class; }
    private static boolean isChain(Target t, String[] cls, int i) { return t.ids[i] != 0 && cls[i] == null && isChainBlock(t.ids[i]); }
    /** 0 = x, 1 = y, 2 = z (the sim reads the state's name: axis=x / axis=z, else y). */
    static int chainAxis(int s) {
        var st = StateInfo.state[s];
        if (!st.hasProperty(net.minecraft.world.level.block.state.properties.BlockStateProperties.AXIS)) return 1;
        var a = st.getValue(net.minecraft.world.level.block.state.properties.BlockStateProperties.AXIS);
        return a == net.minecraft.core.Direction.Axis.X ? 0 : a == net.minecraft.core.Direction.Axis.Z ? 2 : 1;
    }
    /** The two neighbours of cell i along axis ax inside the target box (-1 where there is none). */
    private static int[] chainNbrs(int i, int ax, int sx, int sy, int sz) {
        int x = i % sx, y = i / (sx * sz), z = (i / sx) % sz, plane = sx * sz;
        switch (ax) {
            case 0: return new int[] {x > 0 ? i - 1 : -1, x + 1 < sx ? i + 1 : -1};
            case 2: return new int[] {z > 0 ? i - sx : -1, z + 1 < sz ? i + sx : -1};
            default: return new int[] {y > 0 ? i - plane : -1, y + 1 < sy ? i + plane : -1};
        }
    }

    private static final class IntArrayListLite {
        int[] a = new int[16]; int n;
        void add(int v) { if (n == a.length) a = java.util.Arrays.copyOf(a, n * 2); a[n++] = v; }
    }

    /** @param world the built region's states, same layout as target.ids */
    public static Result judge(Target t, char[] world) { return judge(t, world, null); }

    /** @param base a real level for the impossible-cell view (see {@link #impossibleCells}) */
    public static Result judge(Target t, char[] world, net.minecraft.world.level.LevelReader base) {
        StateInfo.init();
        Result r = new Result();
        String[] imp = IMPOSSIBLE ? impossibleCells(t, base) : null;
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
                        if (!StateInfo.decorativeWide(a) && (w == 0 || !rightCell(a, w))) badW++;
                    }
                    // (exempt: decoration, and - from 2026-11-14, the owner's decision - the blocks only an operator in
                    // creative mode can place: command blocks, structure block, jigsaw, test blocks. Their cells are
                    // neither missing nor part of what the 3 % is taken of. MCB_OLD_JUDGE=1 counts the latter as before.)
                    if (a != 0 && (StateInfo.decorative(a) || (!OLD_JUDGE && StateInfo.gameMaster(a)))) { r.exemptCells++; continue; }
                    // (impossible cells, the owner's decision of 2026-11-20, second form: counted as standing right
                    // whatever stands there - part of the section's count, never bad - so the rule can only help)
                    if (a != 0 && imp != null && imp[i] != null) { r.exemptCells++; tgt++; continue; }
                    if (a == 0) {
                        // (a piston's head where the target shows the piston drawn in is part of that piston, not a block left behind)
                        // (nor the upper half of a door or the head of a bed where the target only has the other part)
                        if (w != 0 && !StateInfo.fluid[w] && !StateInfo.technical[w] && !StateInfo.secondary[w]) { bad++; r.leftover++; note(r, "leftover " + StateInfo.name(w), x, y, z); }
                        continue;
                    }
                    tgt++;
                    if (w == 0) { bad++; r.missing++; note(r, "missing " + StateInfo.name(a), x, y, z); }
                    else if (!rightCell(a, w)) { bad++; r.wrong++; note(r, "wrong " + StateInfo.name(a) + " <- " + StateInfo.name(w), x, y, z); }
                    else if (StateInfo.dewater[w] != a) r.strictMismatch++;
                }
            }
            r.sections++;
            r.sectionList.add(new int[] {cx, cy, cz, tgt, bad});
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
