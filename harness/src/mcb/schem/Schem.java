package mcb.schem;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.zip.Deflater;
import java.util.zip.DeflaterOutputStream;
import java.util.zip.InflaterInputStream;

/**
 * A schematic in normal form: a box of block-state ids (vanilla 26.2 global state ids, 0 = air), cropped to the
 * bounding box of its non-air cells. Index = (y * sz + z) * sx + x.
 */
public final class Schem {
    public final int sx, sy, sz;
    public final char[] ids;
    /** Cells whose block was not a vanilla 26.2 block (modded / unparseable); they are stored as air. */
    public long unknownCells;
    public String format = "?";
    public int dataVersion;

    public Schem(int sx, int sy, int sz, char[] ids) {
        if ((long) sx * sy * sz != ids.length) throw new IllegalArgumentException("size mismatch");
        this.sx = sx; this.sy = sy; this.sz = sz; this.ids = ids;
    }

    public int index(int x, int y, int z) { return (y * sz + z) * sx + x; }
    public int get(int x, int y, int z) { return ids[(y * sz + z) * sx + x]; }
    public long volume() { return (long) sx * sy * sz; }

    public long nonAir() {
        long n = 0;
        for (char c : ids) if (c != 0) n++;
        return n;
    }

    /** Crops to the bounding box of non-air cells. Returns null if everything is air. */
    public Schem cropped() {
        int x0 = sx, x1 = -1, y0 = sy, y1 = -1, z0 = sz, z1 = -1;
        int i = 0;
        for (int y = 0; y < sy; y++) for (int z = 0; z < sz; z++) for (int x = 0; x < sx; x++, i++) {
            if (ids[i] != 0) {
                if (x < x0) x0 = x; if (x > x1) x1 = x;
                if (y < y0) y0 = y; if (y > y1) y1 = y;
                if (z < z0) z0 = z; if (z > z1) z1 = z;
            }
        }
        if (x1 < 0) return null;
        if (x0 == 0 && y0 == 0 && z0 == 0 && x1 == sx - 1 && y1 == sy - 1 && z1 == sz - 1) return this;
        int nx = x1 - x0 + 1, ny = y1 - y0 + 1, nz = z1 - z0 + 1;
        char[] out = new char[nx * ny * nz];
        for (int y = 0; y < ny; y++) for (int z = 0; z < nz; z++) {
            System.arraycopy(ids, ((y + y0) * sz + (z + z0)) * sx + x0, out, (y * nz + z) * nx, nx);
        }
        Schem s = new Schem(nx, ny, nz, out);
        s.unknownCells = unknownCells; s.format = format; s.dataVersion = dataVersion;
        return s;
    }

    /** Content hash (dimensions + states) used to recognise the same build coming from different files. */
    public String contentHash() {
        try {
            MessageDigest md = MessageDigest.getInstance("SHA-256");
            md.update(ByteBuffer.allocate(12).putInt(sx).putInt(sy).putInt(sz).array());
            byte[] buf = new byte[1 << 16];
            int p = 0;
            for (char c : ids) {
                buf[p++] = (byte) (c >> 8); buf[p++] = (byte) c;
                if (p == buf.length) { md.update(buf, 0, p); p = 0; }
            }
            md.update(buf, 0, p);
            byte[] d = md.digest();
            StringBuilder sb = new StringBuilder();
            for (int i = 0; i < 10; i++) sb.append(String.format("%02x", d[i]));
            return sb.toString();
        } catch (Exception e) { throw new RuntimeException(e); }
    }

    private static final int MAGIC = 0x4d434231; // "MCB1"

    public void save(Path file) throws IOException {
        Files.createDirectories(file.getParent());
        Path tmp = file.resolveSibling(file.getFileName() + ".tmp");
        Deflater def = new Deflater(Deflater.BEST_SPEED);
        try (DataOutputStream out = new DataOutputStream(new BufferedOutputStream(new DeflaterOutputStream(Files.newOutputStream(tmp), def, 1 << 16), 1 << 16))) {
            out.writeInt(MAGIC); out.writeInt(sx); out.writeInt(sy); out.writeInt(sz); out.writeLong(unknownCells); out.writeInt(dataVersion); out.writeUTF(format);
            // run-length: (count:varint, id:char)
            int i = 0, n = ids.length;
            while (i < n) {
                char c = ids[i];
                int j = i + 1;
                while (j < n && ids[j] == c) j++;
                int run = j - i;
                while (run >= 0x80) { out.writeByte((run & 0x7f) | 0x80); run >>>= 7; }
                out.writeByte(run);
                out.writeChar(c);
                i = j;
            }
        } finally { def.end(); }
        Files.move(tmp, file, java.nio.file.StandardCopyOption.REPLACE_EXISTING);
    }

    public static Schem load(Path file) throws IOException {
        try (DataInputStream in = new DataInputStream(new BufferedInputStream(new InflaterInputStream(Files.newInputStream(file)), 1 << 16))) {
            if (in.readInt() != MAGIC) throw new IOException("bad magic " + file);
            int sx = in.readInt(), sy = in.readInt(), sz = in.readInt();
            long unknown = in.readLong();
            int dv = in.readInt();
            String fmt = in.readUTF();
            char[] ids = new char[sx * sy * sz];
            int i = 0;
            while (i < ids.length) {
                int run = 0, shift = 0, b;
                do { b = in.readUnsignedByte(); run |= (b & 0x7f) << shift; shift += 7; } while ((b & 0x80) != 0);
                char c = in.readChar();
                if (c != 0) java.util.Arrays.fill(ids, i, i + run, c);
                i += run;
            }
            Schem s = new Schem(sx, sy, sz, ids);
            s.unknownCells = unknown; s.dataVersion = dv; s.format = fmt;
            return s;
        }
    }
}
