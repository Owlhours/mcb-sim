package mcb.test;

import java.nio.file.Paths;
import mcb.env.BotClient;
import mcb.env.Env;
import net.minecraft.core.BlockPos;
import net.minecraft.world.inventory.ContainerInput;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.ChestBlockEntity;
import net.minecraft.world.phys.Vec3;

public class EnvSmoke {
    static void lookAt(BotClient c, Vec3 target) {
        mcb.game.Vec3 e = c.eye();
        double dx = target.x - e.x, dy = target.y - e.y, dz = target.z - e.z;
        double h = Math.sqrt(dx * dx + dz * dz);
        c.look((float) (Math.toDegrees(Math.atan2(dz, dx)) - 90.0), (float) -Math.toDegrees(Math.atan2(dy, h)));
    }

    public static void main(String[] a) throws Exception {
        Env env = Env.boot(Paths.get("/tmp/mcb-smoke"));
        StringBuilder col = new StringBuilder();
        for (int y = env.level.getMinY(); y < env.level.getMinY() + 8; y++) col.append(y).append('=').append(env.level.getBlockState(new BlockPos(0, y, 0)).getBlock().getDescriptionId()).append(' ');
        System.out.println("column at origin: " + col + " maxY=" + env.level.getMaxY());
        int gy = env.level.getMinY();
        while (!env.level.getBlockState(new BlockPos(0, gy, 0)).isAir()) gy++;
        System.out.println("ground top (first air y) = " + gy);
        BotClient c = env.join("Bot", 0.5, gy, 0.5);
        for (int i = 0; i < 5; i++) c.tick();
        System.out.println("pos " + c.player.position() + " onGround=" + c.onGround() + " mode=" + c.player.gameMode.getGameModeForPlayer() + " reach=" + c.reach());

        // --- place a block
        c.player.getInventory().setItem(0, new ItemStack(Items.MOSS_BLOCK, 64));
        c.player.getInventory().setItem(1, new ItemStack(Items.OAK_STAIRS, 64));
        c.selectSlot(0);
        BlockPos support = new BlockPos(2, gy - 1, 0);
        lookAt(c, new Vec3(2.5, gy, 0.5));
        c.clickUse();
        c.tick();
        System.out.println("after place: " + env.level.getBlockState(support.above()) + " changes=" + c.changeCount() + " held=" + c.inventorySlot(0));
        c.tick();
        System.out.println("changes after 2nd tick=" + c.changeCount());
        c.clearChanges();
        // --- stairs facing
        c.selectSlot(1);
        lookAt(c, new Vec3(0.5, gy, 2.5));
        c.clickUse();
        c.tick();
        System.out.println("stairs: " + env.level.getBlockState(new BlockPos(0, gy, 2)));
        // --- break the moss block
        c.selectSlot(0);
        lookAt(c, new Vec3(2.5, gy + 0.5, 0.5));
        c.clickAttack(); c.holdAttack(true);
        int t = 0;
        while (!env.level.getBlockState(support.above()).isAir() && t < 100) { c.tick(); t++; if (t == 1) c.holdAttack(true); }
        c.holdAttack(false);
        System.out.println("moss broken after ticks=" + t + " hit=" + c.lastHit());
        // --- jump trajectory
        c.tick(); c.tick();
        c.jump(true);
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < 14; i++) { c.tick(); if (i == 0) c.jump(false); sb.append(String.format("%.4f ", c.y() - gy)); }
        System.out.println("jump: " + sb);
        // --- sprint speed
        c.look(-90, 0);
        c.keys(true, false, false, false, false, false, true);
        double x0 = 0;
        for (int i = 0; i < 40; i++) { c.tick(); if (i == 29) x0 = c.x(); }
        System.out.println("sprint speed b/tick = " + (c.x() - x0) / 10 + " sprinting=" + c.player.isSprinting());
        c.keys(true, false, false, false, false, true, false);
        for (int i = 0; i < 40; i++) { c.tick(); if (i == 29) x0 = c.x(); }
        System.out.println("sneak speed b/tick = " + (c.x() - x0) / 10 + " pose=" + c.player.getPose() + " eye=" + c.player.getEyeHeight());
        c.releaseKeys();
        for (int i = 0; i < 10; i++) c.tick();
        // --- chest
        BlockPos cp = BlockPos.containing(c.x() + 2, gy, c.z());
        env.level.setBlockAndUpdate(cp, Blocks.CHEST.defaultBlockState());
        ChestBlockEntity be = (ChestBlockEntity) env.level.getBlockEntity(cp);
        be.setItem(0, new ItemStack(Items.STONE_BRICKS, 64));
        be.setItem(5, new ItemStack(Items.GLASS, 33));
        c.tick();
        lookAt(c, Vec3.atCenterOf(cp));
        c.clickUse();
        c.tick();
        System.out.println("container open=" + c.containerOpen() + " menu=" + c.menu().getClass().getSimpleName() + " slots=" + c.menu().slots.size());
        c.clickSlot(0, 0, mcb.game.SlotClick.QUICK_MOVE);
        c.clickSlot(5, 3, mcb.game.SlotClick.SWAP); // swap chest slot 5 into hotbar slot 4 (button 3)
        c.tick(); c.tick();
        c.closeScreen();
        c.tick();
        StringBuilder inv = new StringBuilder();
        for (int i = 0; i < 36; i++) if (!c.inventorySlot(i).isEmpty()) inv.append(i).append(':').append(c.inventorySlot(i)).append(' ');
        System.out.println("open after close=" + c.screenOpen() + " inventory: " + inv);
        // timing
        c.look(90, 0);
        long t0 = System.nanoTime();
        int N = 30000;
        for (int i = 0; i < N; i++) { c.keys(true, false, false, false, false, false, false); if (i % 200 == 0) c.look(i % 400 == 0 ? 90 : -90, 0); c.tick(); }
        long t1 = System.nanoTime();
        for (int i = 0; i < N; i++) { if (i % 200 == 0) c.look(i % 400 == 0 ? 90 : -90, 0); c.tick(); }
        long t2 = System.nanoTime();
        System.out.printf("tick cost: %.1f us (first 30k), %.1f us (next 30k)%n", (t1 - t0) / 1e3 / N, (t2 - t1) / 1e3 / N);
        System.exit(0);
    }
}
