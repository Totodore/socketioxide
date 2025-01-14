import internal from "stream";
import { WebSocket, createWebSocketStream } from "ws";

type WsPacket = ArrayBuffer | Blob | Buffer | Buffer[] | string;

// Wrap WebSocket to provide a async iterator to yield messages
// This is a workaround for the lack of support for async iterators in ws
// It avoids the need to spawn a new event handler + promise for each message
export class WebSocketStream extends WebSocket {
  private stream: internal.Duplex;
  constructor(address: string | URL) {
    super(address);
    this.stream = createWebSocketStream(this, {
      objectMode: true,
      readableObjectMode: true,
    });
    // ignore errors
    this.stream.on("error", () => {});
  }

  // implement async iterator
  async *iterator() {
    for await (const message of this.stream.iterator({
      destroyOnReturn: false,
    })) {
      yield message;
    }
  }
}

export const POLLING_URL = "http://localhost:3000";
export const WS_URL = POLLING_URL.replace("http", "ws");

export const PING_INTERVAL = 300;
export const PING_TIMEOUT = 200;

export function sleep(delay: number) {
  return new Promise((resolve) => setTimeout(resolve, delay));
}

export async function waitForEvent(socket: WebSocketStream, event: string) {
  await new Promise<void>((resolve) => {
    socket.once(event, resolve);
  });
}
export async function waitForMessage<
  T extends ArrayBuffer | Blob | Buffer | Buffer[] | string,
>(socket: WebSocketStream): Promise<T> {
  const { value } = await socket.iterator().next();
  return value as T;
}
export async function waitForPackets(socket: WebSocketStream, count: number) {
  const packets: WsPacket[] = [];
  for await (const packet of socket.iterator()) {
    if (packet.data === "2") {
      // ignore PING packets
      continue;
    }
    packets.push(packet);
    if (packets.length === count) {
      return packets;
    }
  }
  return packets;
}
