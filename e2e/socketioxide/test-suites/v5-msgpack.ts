import assert from "assert";
import { describe, it } from "node:test";
import { decode, encode } from "notepack.io";
import internal from "stream";
import { WebSocket, createWebSocketStream } from "ws";

type WsPacket = ArrayBuffer | Blob | Buffer | Buffer[] | string;

// Wrap WebSocket to provide a async iterator to yield messages
// This is a workaround for the lack of support for async iterators in ws
// It avoids the need to spawn a new event handler + promise for each message
class WebSocketStream extends WebSocket {
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

const URL = "http://localhost:3000";
const WS_URL = URL.replace("http", "ws");

const PING_INTERVAL = 300;
const PING_TIMEOUT = 200;

function sleep(delay: number) {
  return new Promise((resolve) => setTimeout(resolve, delay));
}

async function waitForEvent(socket: WebSocketStream, event: string) {
  await new Promise<void>((resolve) => {
    socket.once(event, resolve);
  });
}
async function waitForMessage<
  T extends ArrayBuffer | Blob | Buffer | Buffer[] | string,
>(socket: WebSocketStream): Promise<T> {
  const { value: data } = await socket.iterator().next();
  return data as T;
}
async function waitForPackets(socket: WebSocketStream, count: number) {
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
}

async function initLongPollingSession() {
  const response = await fetch(`${URL}/socket.io/?EIO=4&transport=polling`);
  const content = await response.text();
  return JSON.parse(content.substring(1)).sid;
}

async function initSocketIOConnection() {
  const socket = new WebSocketStream(
    `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
  );
  socket.binaryType = "arraybuffer";

  await waitForMessage(socket); // Engine.IO handshake

  socket.send(encode({ type: 0, nsp: "/" }));

  await waitForMessage(socket); // Socket.IO handshake
  await waitForMessage(socket); // "auth" packet

  return socket;
}

describe("Engine.IO protocol", () => {
  describe("handshake", () => {
    describe("HTTP long-polling", () => {
      it("should successfully open a session", async () => {
        const response = await fetch(
          `${URL}/socket.io/?EIO=4&transport=polling`,
        );

        assert.equal(response.status, 200);

        const content = await response.text();

        assert.equal(content[0], "0");

        const value = JSON.parse(content.substring(1));
        assert.deepStrictEqual(Object.keys(value).sort(), [
          "maxPayload",
          "pingInterval",
          "pingTimeout",
          "upgrades",
          "sid",
        ]);

        assert.equal(typeof value.sid, "string");
        assert.deepStrictEqual(value.upgrades, ["websocket"]);
        assert.equal(value.pingInterval, PING_INTERVAL);
        assert.equal(value.pingTimeout, PING_TIMEOUT);
        assert.equal(value.maxPayload, 1000000);
      });

      it("should fail with an invalid 'EIO' query parameter", async () => {
        const response = await fetch(`${URL}/socket.io/?transport=polling`);

        assert.equal(response.status, 400);

        const response2 = await fetch(
          `${URL}/socket.io/?EIO=abc&transport=polling`,
        );

        assert.equal(response2.status, 400);
      });

      it("should fail with an invalid 'transport' query parameter", async () => {
        const response = await fetch(`${URL}/socket.io/?EIO=4`);

        assert.equal(response.status, 400);

        const response2 = await fetch(`${URL}/socket.io/?EIO=4&transport=abc`);

        assert.equal(response.status, 400);
      });

      it("should fail with an invalid request method", async () => {
        const response = await fetch(
          `${URL}/socket.io/?EIO=4&transport=polling`,
          {
            method: "post",
          },
        );

        assert.equal(response.status, 400);

        const response2 = await fetch(
          `${URL}/socket.io/?EIO=4&transport=polling`,
          {
            method: "put",
          },
        );

        assert.equal(response.status, 400);
      });
    });

    describe("WebSocket", () => {
      it("should successfully open a session", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
        );

        const data: string = await waitForMessage(socket);

        assert.equal(data, "0");

        const value = JSON.parse(data.substring(1));

        assert.deepStrictEqual(Object.keys(value).sort(), [
          "maxPayload",
          "pingInterval",
          "pingTimeout",
          "upgrades",
          "sid",
        ]);

        assert.equal(typeof value.sid, "string");
        assert.deepStrictEqual(value.upgrades, ["websocket"]);
        assert.equal(value.pingInterval, PING_INTERVAL);
        assert.equal(value.pingTimeout, PING_TIMEOUT);
        assert.equal(value.maxPayload, 1000000);

        socket.close();
      });

      it("should fail with an invalid 'EIO' query parameter", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/socket.io/?transport=websocket`,
        );

        socket.on("error", () => {});

        waitForEvent(socket, "close");

        const socket2 = new WebSocketStream(
          `${WS_URL}/socket.io/?EIO=abc&transport=websocket`,
        );

        socket2.on("error", () => {});

        waitForEvent(socket2, "close");
      });

      it("should fail with an invalid 'transport' query parameter", async () => {
        const socket = new WebSocketStream(`${WS_URL}/socket.io/?EIO=4`);

        socket.on("error", () => {});

        waitForEvent(socket, "close");

        const socket2 = new WebSocketStream(
          `${WS_URL}/socket.io/?EIO=4&transport=abc`,
        );

        socket2.on("error", () => {});

        waitForEvent(socket2, "close");
      });
    });
  });

  describe("heartbeat", function () {
    this.timeout(5000);

    describe("HTTP long-polling", () => {
      it("should send ping/pong packets", async () => {
        const sid = await initLongPollingSession();

        for (let i = 0; i < 3; i++) {
          const pollResponse = await fetch(
            `${URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`,
          );

          assert.equal(pollResponse.status, 200);

          const pollContent = await pollResponse.text();

          assert.equal(pollContent, "2");

          const pushResponse = await fetch(
            `${URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`,
            {
              method: "post",
              body: "3",
            },
          );

          assert.equal(pushResponse.status, 200);
        }
      });

      it("should close the session upon ping timeout", async () => {
        const sid = await initLongPollingSession();

        await sleep(PING_INTERVAL + PING_TIMEOUT);

        const pollResponse = await fetch(
          `${URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`,
        );

        assert.equal(pollResponse.status, 400);
      });
    });

    describe("WebSocket", () => {
      it("should send ping/pong packets", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
        );

        await waitForMessage(socket); // handshake

        for (let i = 0; i < 3; i++) {
          const data = await waitForMessage(socket);

          assert.equal(data, "2");

          socket.send("3");
        }

        socket.close();
      });

      it("should close the session upon ping timeout", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
        );

        await waitForEvent(socket, "close"); // handshake
      });
    });
  });

  describe("close", () => {
    describe("HTTP long-polling", () => {
      it("should forcefully close the session", async () => {
        const sid = await initLongPollingSession();

        const [pollResponse] = await Promise.all([
          fetch(`${URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`),
          fetch(`${URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`, {
            method: "post",
            body: "1",
          }),
        ]);

        assert.equal(pollResponse.status, 200);

        const pullContent = await pollResponse.text();

        assert.equal(pullContent, "6");

        const pollResponse2 = await fetch(
          `${URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`,
        );

        assert.equal(pollResponse2.status, 400);
      });
    });

    describe("WebSocket", () => {
      it("should forcefully close the session", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
        );

        await waitForMessage(socket); // handshake

        socket.send("1");

        await waitForEvent(socket, "close");
      });
    });
  });

  describe("upgrade", () => {
    it("should successfully upgrade from HTTP long-polling to WebSocket", async () => {
      const sid = await initLongPollingSession();

      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket&sid=${sid}`,
      );

      await waitForEvent(socket, "open");

      // send probe
      socket.send("2probe");

      const probeResponse = await waitForMessage(socket);

      assert.equal(probeResponse, "3probe");

      // complete upgrade
      socket.send("5");
    });

    it("should ignore HTTP requests with same sid after upgrade", async () => {
      const sid = await initLongPollingSession();

      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket&sid=${sid}`,
      );

      await waitForEvent(socket, "open");
      socket.send("2probe");
      socket.send("5");

      const pollResponse = await fetch(
        `${URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`,
      );

      assert.equal(pollResponse.status, 400);
    });

    it("should ignore WebSocket connection with same sid after upgrade", async () => {
      const sid = await initLongPollingSession();

      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket&sid=${sid}`,
      );

      await waitForEvent(socket, "open");
      socket.send("2probe");
      socket.send("5");

      const socket2 = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket&sid=${sid}`,
      );

      await waitForEvent(socket2, "close");
    });
  });
});

describe("Socket.IO protocol", () => {
  describe("connect", () => {
    it("should allow connection to the main namespace", async () => {
      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
      );

      await waitForMessage(socket); // Engine.IO handshake

      socket.send(encode({ type: 0, nsp: "/", data: undefined }));

      const data: string = await waitForMessage(socket);
      const handshake = decode(data);
      assert.deepStrictEqual(Object.keys(handshake).sort(), [
        "data",
        "nsp",
        "type",
      ]);
      assert.equal(typeof handshake.data.sid, "string");
      assert.equal(handshake.type, 0);
      assert.equal(handshake.nsp, "/");

      const authPacket = decode(await waitForMessage(socket));
      assert.deepStrictEqual(authPacket, {
        type: 2,
        nsp: "/",
        data: ["auth", null],
      });
    });

    it("should allow connection to the main namespace with a payload", async () => {
      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
      );

      await waitForMessage(socket); // Engine.IO handshake

      socket.send(encode({ type: 0, nsp: "/", data: { token: "123" } }));

      const handshake = decode(await waitForMessage(socket));
      assert.deepStrictEqual(Object.keys(handshake).sort(), [
        "data",
        "nsp",
        "type",
      ]);
      assert.equal(typeof handshake.data.sid, "string");
      assert.equal(handshake.type, 0);
      assert.equal(handshake.nsp, "/");

      const authPacket = decode(await waitForMessage(socket));
      assert.deepStrictEqual(authPacket, {
        type: 2,
        nsp: "/",
        data: ["auth", { token: "123" }],
      });
    });

    it("should allow connection to a custom namespace", async () => {
      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
      );

      await waitForMessage(socket); // Engine.IO handshake
      socket.send(encode({ type: 0, nsp: "/custom" }));

      const handshake = decode(await waitForMessage(socket));
      assert.deepStrictEqual(Object.keys(handshake).sort(), [
        "data",
        "nsp",
        "type",
      ]);
      assert.equal(typeof handshake.data.sid, "string");
      assert.equal(handshake.type, 0);
      assert.equal(handshake.nsp, "/custom");

      const authPacket = decode(await waitForMessage(socket));
      assert.deepStrictEqual(authPacket, {
        type: 2,
        nsp: "/custom",
        data: ["auth", null],
      });
    });

    it("should allow connection to a custom namespace with a payload", async () => {
      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
      );

      await waitForMessage(socket); // Engine.IO handshake
      socket.send(encode({ type: 0, nsp: "/custom", data: { token: "abc" } }));

      const handshake = decode(await waitForMessage(socket));
      assert.deepStrictEqual(Object.keys(handshake).sort(), [
        "data",
        "nsp",
        "type",
      ]);
      assert.equal(typeof handshake.data.sid, "string");
      assert.equal(handshake.type, 0);
      assert.equal(handshake.nsp, "/custom");

      const authPacket = decode(await waitForMessage(socket));
      assert.deepStrictEqual(authPacket, {
        type: 2,
        nsp: "/custom",
        data: ["auth", { token: "abc" }],
      });
    });

    it("should disallow connection to an unknown namespace", async () => {
      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
      );

      await waitForMessage(socket); // Engine.IO handshake
      socket.send(encode({ type: 0, nsp: "/random" }));

      const msg = decode(await waitForMessage(socket));
      assert.deepStrictEqual(msg, {
        type: 4,
        nsp: "/random",
        data: { message: "Invalid namespace" },
      });
    });

    it("should disallow connection with an invalid handshake", async () => {
      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
      );

      await waitForMessage(socket); // Engine.IO handshake

      socket.send(new Uint8Array([4, "a", "b", "c"]));

      await waitForEvent(socket, "close");
    });
  });

  describe("disconnect", () => {
    it("should disconnect from the main namespace", async () => {
      const socket = await initSocketIOConnection();

      socket.send(encode({ type: 1, nsp: "/" }));

      const { data } = await waitForMessage(socket);
      expect(data).to.eql("2");
    });

    it("should connect then disconnect from a custom namespace", async () => {
      const socket = await initSocketIOConnection();

      await waitForMessage(socket); // ping
      socket.send(encode({ type: 0, nsp: "/custom" }));

      await waitForMessage(socket); // Socket.IO handshake
      await waitForMessage(socket); // auth packet
      socket.send(encode({ type: 1, nsp: "/custom" }));
      socket.send(
        encode({
          type: 2,
          nsp: "/",
          data: ["message", "message to main namespace", "test", 1],
        }),
      );
      const a = await waitForMessage(socket);
      const data = decode(Buffer.from(a.data));
      expect(data).to.eql({
        type: 2,
        nsp: "/",
        data: ["message-back", "message to main namespace", "test", 1],
      });
    });
  });

  describe("acknowledgements", () => {
    it("should emit with an ack expectation", async () => {
      const socket = await initSocketIOConnection();
      socket.send(
        encode({
          type: 2,
          nsp: "/",
          data: ["emit-with-ack", 1, "2", { 3: [true] }],
        }),
      );

      const data = decode(Buffer.from((await waitForMessage(socket)).data));
      expect(data).to.eql({
        type: 2,
        nsp: "/",
        id: 1,
        data: ["emit-with-ack", 1, "2", { 3: [true] }],
      });
      socket.send(
        encode({ type: 3, nsp: "/", id: 1, data: [1, "2", { 3: [true] }] }),
      );

      const data2 = decode(Buffer.from((await waitForMessage(socket)).data));
      expect(data2).to.eql({
        type: 2,
        nsp: "/",
        data: ["emit-with-ack", 1, "2", { 3: [true] }],
      });
    });

    it("should emit with a binary ack expectation", async () => {
      const socket = await initSocketIOConnection();
      const BINS = [Buffer.from([1, 2, 3]), Buffer.from([4, 5, 6])];
      socket.send(
        encode({
          type: 2,
          nsp: "/",
          data: ["emit-with-ack", ...BINS, "test"],
        }),
      );

      let packet = decode(Buffer.from((await waitForMessage(socket)).data));
      expect(packet).to.eql({
        type: 2,
        nsp: "/",
        id: 1,
        data: ["emit-with-ack", ...BINS, "test"],
      });

      socket.send(
        encode({ type: 3, nsp: "/", id: 1, data: [...BINS, "test"] }),
      );

      packet = decode(Buffer.from((await waitForMessage(socket)).data));
      expect(packet).to.eql({
        type: 2,
        nsp: "/",
        data: ["emit-with-ack", ...BINS, "test"],
      });
    });
  });

  describe("message", () => {
    it("should send a plain-text packet", async () => {
      const socket = await initSocketIOConnection();

      socket.send(
        encode({
          type: 2,
          nsp: "/",
          data: ["message", 1, "2", { 3: [true] }],
        }),
      );

      const data = decode(Buffer.from((await waitForMessage(socket)).data));
      expect(data).to.eql({
        type: 2,
        nsp: "/",
        data: ["message-back", 1, "2", { 3: [true] }],
      });
    });

    it("should send a packet with binary attachments", async () => {
      const socket = await initSocketIOConnection();
      const BINS = [Buffer.from([1, 2, 3]), Buffer.from([4, 5, 6])];

      socket.send(
        encode({
          type: 2,
          nsp: "/",
          data: ["message", ...BINS, "test"],
        }),
      );
      const data = decode(Buffer.from((await waitForMessage(socket)).data));
      expect(data).to.eql({
        type: 2,
        nsp: "/",
        data: ["message-back", ...BINS, "test"],
      });

      socket.close();
    });

    it("should send a plain-text packet with an ack", async () => {
      const socket = await initSocketIOConnection();
      socket.send(
        encode({
          type: 2,
          id: 456,
          nsp: "/",
          data: ["message-with-ack", 1, "2", { 3: [false] }],
        }),
      );

      const data = decode(Buffer.from((await waitForMessage(socket)).data));
      expect(data).to.eql({
        type: 3,
        id: 456,
        nsp: "/",
        data: [1, "2", { 3: [false] }],
      });
    });

    it("should send a packet with binary attachments and an ack", async () => {
      const socket = await initSocketIOConnection();
      const BINS = [Buffer.from([1, 2, 3]), Buffer.from([4, 5, 6])];

      socket.send(
        encode({
          type: 2,
          nsp: "/",
          id: 789,
          data: ["message-with-ack", ...BINS, "test"],
        }),
      );

      const data = decode(Buffer.from((await waitForMessage(socket)).data));
      expect(data).to.eql({
        type: 3,
        id: 789,
        data: [...BINS, "test"],
        nsp: "/",
      });

      socket.close();
    });

    it("should close the connection upon invalid format (unknown packet type)", async () => {
      const socket = await initSocketIOConnection();
      socket.send("4abc");
      socket.send(Buffer.from([1, 2, 3, 4]));

      await waitForEvent(socket, "close");
    });

    it("should close the connection upon invalid format (invalid payload format)", async () => {
      const socket = await initSocketIOConnection();

      socket.send("42{}");

      await waitForEvent(socket, "close");
    });

    it("should close the connection upon invalid format (invalid ack id)", async () => {
      const socket = await initSocketIOConnection();

      socket.send('42abc["message-with-ack",1,"2",{"3":[false]}]');

      await waitForEvent(socket, "close");
    });
  });
});
