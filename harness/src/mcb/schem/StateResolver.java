package mcb.schem;

import com.mojang.datafixers.DataFixer;
import com.mojang.serialization.Dynamic;
import java.util.HashMap;
import java.util.Map;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.NbtUtils;
import net.minecraft.nbt.Tag;
import net.minecraft.resources.Identifier;
import net.minecraft.util.datafix.DataFixers;
import net.minecraft.util.datafix.fixes.BlockStateData;
import net.minecraft.util.datafix.fixes.References;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;

/**
 * Turns block states as written in schematic files (any game version) into 26.2 state ids, using the game's own
 * data fixers for the version upgrade and its own parser for the result. Returns -1 for blocks that do not exist
 * in vanilla 26.2 (mod blocks).
 */
public final class StateResolver {
    public static final int CURRENT = 4903;
    private static DataFixer fixer;
    private final Map<String, Integer> cache = new HashMap<>();

    private static DataFixer fixer() {
        if (fixer == null) fixer = DataFixers.getDataFixer();
        return fixer;
    }

    /** {Name, Properties} compound as found in litematic / structure palettes. */
    public int resolve(CompoundTag state, int dataVersion) {
        String key = dataVersion + "|" + state;
        Integer c = cache.get(key);
        if (c != null) return c;
        int id = resolveUncached(state.copy(), dataVersion);
        cache.put(key, id);
        return id;
    }

    /** "minecraft:oak_stairs[facing=north,half=top]" as found in Sponge palettes. */
    public int resolve(String spec, int dataVersion) {
        String key = dataVersion + "|" + spec;
        Integer c = cache.get(key);
        if (c != null) return c;
        int id = resolveUncached(parseSpec(spec), dataVersion);
        cache.put(key, id);
        return id;
    }

    /** Pre-1.13 numeric block id + data nibble (MCEdit .schematic). */
    public int resolveLegacy(int blockId, int data) {
        String key = "L|" + blockId + ":" + data;
        Integer c = cache.get(key);
        if (c != null) return c;
        Dynamic<?> d = BlockStateData.getTag((blockId << 4) | (data & 15));
        Tag t = d.convert(NbtOps.INSTANCE).getValue();
        int id = t instanceof CompoundTag ct ? resolveUncached(ct.copy(), 1451) : -1;
        if (blockId != 0 && id == 0) id = -1; // the fixer's table answers "air" for ids it does not know
        cache.put(key, id);
        return id;
    }

    static CompoundTag parseSpec(String spec) {
        CompoundTag t = new CompoundTag();
        int b = spec.indexOf('[');
        String name = b < 0 ? spec : spec.substring(0, b);
        if (name.indexOf(':') < 0) name = "minecraft:" + name;
        t.putString("Name", name.trim());
        if (b >= 0) {
            int e = spec.lastIndexOf(']');
            if (e < b) e = spec.length();
            CompoundTag props = new CompoundTag();
            for (String kv : spec.substring(b + 1, e).split(",")) {
                int eq = kv.indexOf('=');
                if (eq > 0) props.putString(kv.substring(0, eq).trim(), kv.substring(eq + 1).trim());
            }
            if (!props.isEmpty()) t.put("Properties", props);
        }
        return t;
    }

    private int resolveUncached(CompoundTag tag, int dataVersion) {
        try {
            if (dataVersion > 0 && dataVersion < CURRENT) {
                Dynamic<Tag> upgraded = fixer().update(References.BLOCK_STATE, new Dynamic<>(NbtOps.INSTANCE, tag), dataVersion, CURRENT);
                if (upgraded.getValue() instanceof CompoundTag ct) tag = ct;
            }
            String name = tag.getStringOr("Name", "");
            Identifier id = Identifier.tryParse(name);
            if (id == null || !BuiltInRegistries.BLOCK.containsKey(id)) return -1;
            BlockState st = NbtUtils.readBlockState(BuiltInRegistries.BLOCK, tag);
            return Block.getId(st);
        } catch (RuntimeException e) {
            return -1;
        }
    }
}
