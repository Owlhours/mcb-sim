package mcb.test;

import java.nio.file.Paths;
import mcb.env.BotClient;
import mcb.env.Env;
import net.minecraft.core.BlockPos;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.block.Blocks;

/** Checks how scaffolding blocks behave for the bot: putting up a column from inside, climbing, sinking, collapse. */
public class TowerTest {
    static Env env; static BotClient c; static int gy;
    static String col(int x, int z, int n) {
        StringBuilder sb = new StringBuilder();
        for (int y = gy; y < gy + n; y++) sb.append(env.level.getBlockState(new BlockPos(x, y, z)).is(Blocks.SCAFFOLDING) ? 'S' : env.level.getBlockState(new BlockPos(x, y, z)).isAir() ? '.' : '#');
        return sb.toString();
    }
    static void say(String what) { System.out.println(String.format("%-34s t=%d y=%.3f ground=%s col=%s", what, c.ticks, c.y() - gy, c.onGround(), col(8, 8, 12))); }

    public static void main(String[] a) throws Exception {
        env = Env.boot(Paths.get("/tmp/mcb-tower"));
        gy = env.level.getMinY();
        while (!env.level.getBlockState(new BlockPos(8, gy, 8)).isAir()) gy++;
        c = env.join("Bot", 8.5, gy, 8.5);
        for (int i = 0; i < 5; i++) c.tick();
        c.player.getInventory().setItem(0, new ItemStack(Items.SCAFFOLDING, 64));
        c.player.getInventory().setItem(1, new ItemStack(Items.MOSS_BLOCK, 64));
        c.selectSlot(0);
        c.tick();
        // 1. first block into the own cell, clicking the floor
        c.look(0, 90); c.clickUse(); c.tick(); c.tick();
        say("first (click floor)");
        // 2. second: sneak + click the top of the first
        c.sneak(true); c.tick(); c.tick();
        c.look(0, 90); c.clickUse(); c.tick(); c.tick();
        c.sneak(false); c.tick();
        say("second (sneak, click top)");
        // 3. more from inside, looking up
        for (int i = 0; i < 6; i++) { c.look(0, -90); c.clickUse(); c.tick(); c.tick(); c.tick(); c.tick(); say("  look up + click " + i); }
        // 4. climb
        long t0 = c.ticks;
        c.jump(true);
        while (c.y() < gy + 8 && c.ticks - t0 < 200) c.tick();
        c.jump(false);
        for (int i = 0; i < 10; i++) c.tick();
        say("climbed in " + (c.ticks - t0 - 10));
        // 5. sink three levels
        t0 = c.ticks;
        c.sneak(true);
        while (c.y() > gy + 5 + 0.15 && c.ticks - t0 < 200) c.tick();
        c.sneak(false);
        for (int i = 0; i < 10; i++) c.tick();
        say("sank 3 in " + (c.ticks - t0 - 10));
        // 6. one sneak-click while standing on scaffolding (how far does the bot sink?)
        c.sneak(true); c.tick(); c.sneak(false);
        for (int i = 0; i < 10; i++) c.tick();
        say("after one tick of sneak");
        // 7. grow from mid-column, looking level
        c.look(0, 0); c.clickUse(); c.tick(); c.tick();
        say("click looking level");
        // 8. to the bottom, then break the block at head height: everything above should go
        c.sneak(true);
        t0 = c.ticks;
        while (c.y() > gy + 0.01 && c.ticks - t0 < 300) c.tick();
        c.sneak(false);
        for (int i = 0; i < 5; i++) c.tick();
        say("at the bottom after " + (c.ticks - t0));
        c.selectSlot(1); c.tick();
        c.look(0, -90); c.clickAttack(); c.tick();
        say("attack up, hit=" + c.lastHit());
        for (int i = 0; i < 30; i++) { c.tick(); if (i % 3 == 0) say("  collapse +" + (i + 1)); }
        c.look(0, 90); c.clickAttack(); c.tick(); c.tick();
        say("attack down, hit=" + (c.lastHit() == null ? null : c.lastHit().pos()));
        System.out.println("entities: " + env.level.getAllEntities().spliterator().estimateSize());
        System.exit(0);
    }
}
