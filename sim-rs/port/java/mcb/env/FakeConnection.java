package mcb.env;

import io.netty.channel.ChannelFutureListener;
import java.net.InetSocketAddress;
import java.net.SocketAddress;
import java.util.function.Consumer;
import net.minecraft.network.Connection;
import net.minecraft.network.PacketListener;
import net.minecraft.network.ProtocolInfo;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.PacketFlow;

/**
 * A network connection with no socket behind it. Everything the server sends to the "client" is handed to
 * {@link #sink}; nothing is ever received (the bot's inputs are delivered by calling the server's packet
 * handlers directly, see {@link Client}).
 */
public final class FakeConnection extends Connection {
    public Consumer<Packet<?>> sink = p -> { };
    public long sent;

    public FakeConnection() { super(PacketFlow.SERVERBOUND); }

    @Override public void send(Packet<?> p) { sent++; sink.accept(p); }
    @Override public void send(Packet<?> p, ChannelFutureListener l) { sent++; sink.accept(p); }
    @Override public void send(Packet<?> p, ChannelFutureListener l, boolean flush) { sent++; sink.accept(p); }
    @Override public <T extends PacketListener> void setupInboundProtocol(ProtocolInfo<T> info, T listener) { }
    @Override public void setupOutboundProtocol(ProtocolInfo<?> info) { }
    @Override public void flushChannel() { }
    @Override public void tick() { }
    @Override public boolean isConnected() { return true; }
    @Override public boolean isConnecting() { return false; }
    @Override public void setReadOnly() { }
    @Override public void handleDisconnection() { }
    @Override public SocketAddress getRemoteAddress() { return new InetSocketAddress("127.0.0.1", 0); }
    @Override public boolean isMemoryConnection() { return true; }
}
