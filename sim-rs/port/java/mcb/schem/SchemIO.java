package mcb.schem;

import java.io.IOException;
import java.nio.file.Path;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.NbtAccounter;
import net.minecraft.nbt.NbtIo;
import net.minecraft.nbt.Tag;

/** Readers for the schematic formats found in the collection. All return a cropped {@link Schem} or throw. */
public final class SchemIO {
    public static final long MAX_VOLUME = 400_000_000L;
    private final StateResolver resolver = new StateResolver();

    public Schem read(Path file) throws IOException {
        CompoundTag root = NbtIo.readCompressed(file, NbtAccounter.unlimitedHeap());
        String name = file.getFileName().toString().toLowerCase();
        Schem s;
        if (root.contains("Regions")) s = litematic(root);
        else if (root.contains("Schematic") || root.contains("BlockData") || root.contains("Palette") && root.contains("Width")) s = sponge(root);
        else if (root.contains("Blocks") && root.contains("Width") && root.get("Blocks").getId() == Tag.TAG_BYTE_ARRAY) s = legacy(root);
        else if (root.contains("Blocks") && root.contains("Width")) s = sponge(root);
        else if (root.contains("size") && (root.contains("palette") || root.contains("palettes"))) s = structure(root);
        else throw new IOException("unrecognised schematic layout (" + name + "): keys " + root.keySet());
        Schem c = s.cropped();
        if (c == null) throw new IOException("no blocks");
        return c;
    }

    private static void checkDims(long sx, long sy, long sz) throws IOException {
        if (sx <= 0 || sy <= 0 || sz <= 0) throw new IOException("invalid dimensions " + sx + "x" + sy + "x" + sz);
        if (sx * sy * sz > MAX_VOLUME) throw new IOException("volume too large: " + sx + "x" + sy + "x" + sz);
    }

    // ---------------------------------------------------------------- Sponge v1/v2/v3
    private Schem sponge(CompoundTag root) throws IOException {
        if (root.contains("Schematic")) root = root.getCompoundOrEmpty("Schematic");
        int sx = root.getShortOr("Width", (short) 0) & 0xffff, sy = root.getShortOr("Height", (short) 0) & 0xffff, sz = root.getShortOr("Length", (short) 0) & 0xffff;
        checkDims(sx, sy, sz);
        int dv = root.getIntOr("DataVersion", 0);
        CompoundTag palette;
        byte[] data;
        if (root.contains("Blocks") && root.get("Blocks") instanceof CompoundTag blocks) { // v3
            palette = blocks.getCompoundOrEmpty("Palette");
            data = blocks.getByteArray("Data").orElseThrow(() -> new IOException("no block data"));
        } else {
            palette = root.getCompoundOrEmpty("Palette");
            data = root.getByteArray("BlockData").orElseThrow(() -> new IOException("no block data"));
        }
        if (dv <= 0) dv = 1519; // earliest version that wrote this format (1.13)
        int max = 0;
        for (String k : palette.keySet()) max = Math.max(max, palette.getIntOr(k, 0));
        int[] map = new int[max + 1];
        java.util.Arrays.fill(map, -1);
        for (String k : palette.keySet()) map[palette.getIntOr(k, 0)] = resolver.resolve(k, dv);
        char[] ids = new char[sx * sy * sz];
        long unknown = 0;
        int p = 0;
        for (int i = 0; i < ids.length; i++) {
            if (p >= data.length) throw new IOException("block data truncated");
            int v = 0, shift = 0, b;
            do { b = data[p++]; v |= (b & 0x7f) << shift; shift += 7; } while ((b & 0x80) != 0 && p < data.length);
            int id = v >= 0 && v < map.length ? map[v] : -1;
            if (id < 0) unknown++; else ids[i] = (char) id;
        }
        Schem s = new Schem(sx, sy, sz, ids);
        s.unknownCells = unknown; s.format = "sponge"; s.dataVersion = dv;
        return s;
    }

    // ---------------------------------------------------------------- Litematica
    private Schem litematic(CompoundTag root) throws IOException {
        int dv = root.getIntOr("MinecraftDataVersion", 0);
        if (dv <= 0) dv = 1343;
        CompoundTag regions = root.getCompoundOrEmpty("Regions");
        if (regions.isEmpty()) throw new IOException("no regions");
        long minX = Long.MAX_VALUE, minY = Long.MAX_VALUE, minZ = Long.MAX_VALUE, maxX = Long.MIN_VALUE, maxY = Long.MIN_VALUE, maxZ = Long.MIN_VALUE;
        for (String rn : regions.keySet()) {
            CompoundTag r = regions.getCompoundOrEmpty(rn);
            CompoundTag pos = r.getCompoundOrEmpty("Position"), size = r.getCompoundOrEmpty("Size");
            int[] p = {pos.getIntOr("x", 0), pos.getIntOr("y", 0), pos.getIntOr("z", 0)}, s = {size.getIntOr("x", 0), size.getIntOr("y", 0), size.getIntOr("z", 0)};
            if (s[0] == 0 || s[1] == 0 || s[2] == 0) continue;
            int x0 = s[0] < 0 ? p[0] + s[0] + 1 : p[0], y0 = s[1] < 0 ? p[1] + s[1] + 1 : p[1], z0 = s[2] < 0 ? p[2] + s[2] + 1 : p[2];
            minX = Math.min(minX, x0); minY = Math.min(minY, y0); minZ = Math.min(minZ, z0);
            maxX = Math.max(maxX, x0 + Math.abs(s[0]) - 1); maxY = Math.max(maxY, y0 + Math.abs(s[1]) - 1); maxZ = Math.max(maxZ, z0 + Math.abs(s[2]) - 1);
        }
        if (minX == Long.MAX_VALUE) throw new IOException("empty regions");
        long sxl = maxX - minX + 1, syl = maxY - minY + 1, szl = maxZ - minZ + 1;
        checkDims(sxl, syl, szl);
        int sx = (int) sxl, sy = (int) syl, sz = (int) szl;
        char[] ids = new char[sx * sy * sz];
        long unknown = 0;
        for (String rn : regions.keySet()) {
            CompoundTag r = regions.getCompoundOrEmpty(rn);
            CompoundTag pos = r.getCompoundOrEmpty("Position"), size = r.getCompoundOrEmpty("Size");
            int[] p = {pos.getIntOr("x", 0), pos.getIntOr("y", 0), pos.getIntOr("z", 0)}, s = {size.getIntOr("x", 0), size.getIntOr("y", 0), size.getIntOr("z", 0)};
            if (s[0] == 0 || s[1] == 0 || s[2] == 0) continue;
            int rx = Math.abs(s[0]), ry = Math.abs(s[1]), rz = Math.abs(s[2]);
            int ox = (int) ((s[0] < 0 ? p[0] + s[0] + 1 : p[0]) - minX), oy = (int) ((s[1] < 0 ? p[1] + s[1] + 1 : p[1]) - minY), oz = (int) ((s[2] < 0 ? p[2] + s[2] + 1 : p[2]) - minZ);
            ListTag pal = r.getListOrEmpty("BlockStatePalette");
            int[] map = new int[pal.size()];
            for (int i = 0; i < map.length; i++) map[i] = resolver.resolve(pal.getCompoundOrEmpty(i), dv);
            long[] packed = r.getLongArray("BlockStates").orElseThrow(() -> new IOException("no BlockStates"));
            int bits = Math.max(2, 32 - Integer.numberOfLeadingZeros(Math.max(1, map.length - 1)));
            long mask = (1L << bits) - 1;
            long total = (long) rx * ry * rz;
            if ((total * bits + 63) / 64 > packed.length) throw new IOException("BlockStates truncated");
            long idx = 0;
            for (int y = 0; y < ry; y++) for (int z = 0; z < rz; z++) {
                int base = ((y + oy) * sz + (z + oz)) * sx + ox;
                for (int x = 0; x < rx; x++, idx++) {
                    long bit = idx * bits;
                    int li = (int) (bit >>> 6), off = (int) (bit & 63);
                    long v = packed[li] >>> off;
                    if (off + bits > 64) v |= packed[li + 1] << (64 - off);
                    int pi = (int) (v & mask);
                    int id = pi < map.length ? map[pi] : -1;
                    if (id < 0) unknown++; else if (id != 0) ids[base + x] = (char) id;
                }
            }
        }
        Schem s = new Schem(sx, sy, sz, ids);
        s.unknownCells = unknown; s.format = "litematic"; s.dataVersion = dv;
        return s;
    }

    // ---------------------------------------------------------------- vanilla structure .nbt
    private Schem structure(CompoundTag root) throws IOException {
        ListTag size = root.getListOrEmpty("size");
        int sx = size.getIntOr(0, 0), sy = size.getIntOr(1, 0), sz = size.getIntOr(2, 0);
        checkDims(sx, sy, sz);
        int dv = root.getIntOr("DataVersion", 0);
        if (dv <= 0) dv = 500;
        ListTag pal = root.contains("palette") ? root.getListOrEmpty("palette") : root.getListOrEmpty("palettes").getListOrEmpty(0);
        int[] map = new int[pal.size()];
        for (int i = 0; i < map.length; i++) {
            CompoundTag st = pal.getCompoundOrEmpty(i);
            map[i] = st.getStringOr("Name", "").equals("minecraft:structure_void") ? 0 : resolver.resolve(st, dv);
        }
        char[] ids = new char[sx * sy * sz];
        long unknown = 0;
        ListTag blocks = root.getListOrEmpty("blocks");
        for (int i = 0; i < blocks.size(); i++) {
            CompoundTag b = blocks.getCompoundOrEmpty(i);
            ListTag pos = b.getListOrEmpty("pos");
            int x = pos.getIntOr(0, -1), y = pos.getIntOr(1, -1), z = pos.getIntOr(2, -1);
            if (x < 0 || y < 0 || z < 0 || x >= sx || y >= sy || z >= sz) continue;
            int pi = b.getIntOr("state", -1);
            int id = pi >= 0 && pi < map.length ? map[pi] : -1;
            if (id < 0) unknown++; else ids[(y * sz + z) * sx + x] = (char) id;
        }
        Schem s = new Schem(sx, sy, sz, ids);
        s.unknownCells = unknown; s.format = "structure"; s.dataVersion = dv;
        return s;
    }

    // ---------------------------------------------------------------- MCEdit .schematic (numeric ids)
    private Schem legacy(CompoundTag root) throws IOException {
        int sx = root.getShortOr("Width", (short) 0) & 0xffff, sy = root.getShortOr("Height", (short) 0) & 0xffff, sz = root.getShortOr("Length", (short) 0) & 0xffff;
        checkDims(sx, sy, sz);
        byte[] blocks = root.getByteArray("Blocks").orElseThrow(() -> new IOException("no Blocks"));
        byte[] data = root.getByteArray("Data").orElse(new byte[blocks.length]);
        byte[] add = root.getByteArray("AddBlocks").orElse(null);
        int n = sx * sy * sz;
        if (blocks.length < n || data.length < n) throw new IOException("legacy arrays truncated");
        char[] ids = new char[n];
        long unknown = 0;
        for (int i = 0; i < n; i++) {
            int id = blocks[i] & 0xff;
            if (add != null && (i >> 1) < add.length) id |= ((i & 1) == 0 ? (add[i >> 1] & 0x0f) : ((add[i >> 1] >> 4) & 0x0f)) << 8;
            if (id == 0) continue;
            int st = resolver.resolveLegacy(id, data[i] & 15);
            if (st < 0) unknown++; else ids[i] = (char) st;
        }
        Schem s = new Schem(sx, sy, sz, ids);
        s.unknownCells = unknown; s.format = "legacy"; s.dataVersion = 1343;
        return s;
    }
}
