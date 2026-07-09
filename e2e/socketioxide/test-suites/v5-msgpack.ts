import assert from "assert";
import { describe, it } from "node:test";
import { decode, encode } from "notepack.io";
import {
  PING_INTERVAL,
  PING_TIMEOUT,
  sleep,
  waitForEvent,
  waitForMessage,
  WebSocketStream,
  WS_URL,
  POLLING_URL,
} from "./utils.ts";

async function initLongPollingSession() {
  const response = await fetch(
    `${POLLING_URL}/socket.io/?EIO=4&transport=polling`,
  );
  const content = await response.text();
  const sid = JSON.parse(content.substring(1)).sid;
  // receive ping packet
  const pingRes = await fetch(
    `${POLLING_URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`,
  );
  assert.equal(pingRes.status, 200);
  const ping = await pingRes.text();
  assert.equal(ping, "2");

  return sid;
}

async function initSocketIOConnection() {
  const socket = new WebSocketStream(
    `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
  );
  socket.binaryType = "arraybuffer";

  await waitForMessage(socket); // Engine.IO handshake
  await waitForMessage(socket); // Engine.IO ping

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
          `${POLLING_URL}/socket.io/?EIO=4&transport=polling`,
        );

        assert.equal(response.status, 200);

        const content = await response.text();

        assert.equal(content[0], "0");

        const value = JSON.parse(content.substring(1));
        assert.deepStrictEqual(Object.keys(value).sort(), [
          "maxPayload",
          "pingInterval",
          "pingTimeout",
          "sid",
          "upgrades",
        ]);

        assert.equal(typeof value.sid, "string");
        assert.deepStrictEqual(value.upgrades, ["websocket"]);
        assert.equal(value.pingInterval, PING_INTERVAL);
        assert.equal(value.pingTimeout, PING_TIMEOUT);
        assert.equal(value.maxPayload, 1000000);
      });

      it("should fail with an invalid 'EIO' query parameter", async () => {
        const response = await fetch(
          `${POLLING_URL}/socket.io/?transport=polling`,
        );

        assert.equal(response.status, 400);

        const response2 = await fetch(
          `${POLLING_URL}/socket.io/?EIO=abc&transport=polling`,
        );

        assert.equal(response2.status, 400);
      });

      it("should fail with an invalid 'transport' query parameter", async () => {
        const response = await fetch(`${POLLING_URL}/socket.io/?EIO=4`);

        assert.equal(response.status, 400);

        const response2 = await fetch(
          `${POLLING_URL}/socket.io/?EIO=4&transport=abc`,
        );

        assert.equal(response.status, 400);
      });

      it("should fail with an invalid request method", async () => {
        const response = await fetch(
          `${POLLING_URL}/socket.io/?EIO=4&transport=polling`,
          {
            method: "post",
          },
        );

        assert.equal(response.status, 400);

        const response2 = await fetch(
          `${POLLING_URL}/socket.io/?EIO=4&transport=polling`,
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

        assert.equal(data[0], "0");

        const value = JSON.parse(data.substring(1));

        assert.deepStrictEqual(Object.keys(value).sort(), [
          "maxPayload",
          "pingInterval",
          "pingTimeout",
          "sid",
          "upgrades",
        ]);

        assert.equal(typeof value.sid, "string");
        assert.deepStrictEqual(value.upgrades, []);
        assert.equal(value.pingInterval, PING_INTERVAL);
        assert.equal(value.pingTimeout, PING_TIMEOUT);
        assert.equal(value.maxPayload, 1000000);

        socket.close();
      });

      it("should fail with an invalid 'EIO' query parameter", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/socket.io/?transport=websocket`,
        );

        socket.on("error", () => { });

        waitForEvent(socket, "close");

        const socket2 = new WebSocketStream(
          `${WS_URL}/socket.io/?EIO=abc&transport=websocket`,
        );

        socket2.on("error", () => { });

        waitForEvent(socket2, "close");
      });

      it("should fail with an invalid 'transport' query parameter", async () => {
        const socket = new WebSocketStream(`${WS_URL}/socket.io/?EIO=4`);

        socket.on("error", () => { });

        waitForEvent(socket, "close");

        const socket2 = new WebSocketStream(
          `${WS_URL}/socket.io/?EIO=4&transport=abc`,
        );

        socket2.on("error", () => { });

        waitForEvent(socket2, "close");
      });
    });
  });

  describe("heartbeat", { timeout: 5000 }, function () {
    describe("HTTP long-polling", () => {
      it("should send ping/pong packets", async () => {
        const response = await fetch(
          `${POLLING_URL}/socket.io/?EIO=4&transport=polling`,
        );
        const content = await response.text();
        const sid = JSON.parse(content.substring(1)).sid;

        for (let i = 0; i < 3; i++) {
          const pollResponse = await fetch(
            `${POLLING_URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`,
          );

          assert.equal(pollResponse.status, 200);

          const pollContent = await pollResponse.text();

          assert.equal(pollContent, "2");

          const pushResponse = await fetch(
            `${POLLING_URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`,
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
          `${POLLING_URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`,
        );

        assert(
          pollResponse.status == 400 ||
          (pollResponse.status == 200 && (await pollResponse.text()) == "1"),
        );
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
          fetch(`${POLLING_URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`),
          fetch(
            `${POLLING_URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`,
            {
              method: "post",
              body: "1",
            },
          ),
        ]);

        assert.equal(pollResponse.status, 200);

        const pullContent = await pollResponse.text();

        assert.equal(pullContent, "6");

        const pollResponse2 = await fetch(
          `${POLLING_URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`,
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
        `${POLLING_URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`,
      );

      assert.equal(pollResponse.status, 400);
    });


    it("should NOOP all polling requests while upgrading", async () => {
      const sid = await initLongPollingSession();

      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket&sid=${sid}`,
      );
      await waitForEvent(socket, "open");
      for (let i = 0; i < 5; i++) {
        const res = await fetch(`${POLLING_URL}/socket.io/?EIO=4&transport=polling&sid=${sid}`);
        assert.deepStrictEqual(res.status, 200);
        const body = await res.text();
        assert.deepStrictEqual(body, "6");
      }

      socket.send("2probe");
      socket.send("5");

      await waitForMessage(socket); // 3probe
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
      await waitForMessage(socket); // Engine.IO ping

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
      await waitForMessage(socket); // Engine.IO ping

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
      await waitForMessage(socket); // Engine.IO ping
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
      await waitForMessage(socket); // Engine.IO ping

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
      await waitForMessage(socket); // Engine.IO ping
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
      await waitForMessage(socket); // Engine.IO ping

      socket.send(new Uint8Array([4, 132, 23, 2]));

      await waitForEvent(socket, "close");
    });
  });

  describe("disconnect", () => {
    it("should disconnect from the main namespace", async () => {
      const socket = await initSocketIOConnection();

      socket.send(encode({ type: 1, nsp: "/" }));

      // The socket heartbeat should timeout.
      await waitForEvent(socket, "close");
    });

    it("should connect then disconnect from a custom namespace", async () => {
      const socket = await initSocketIOConnection();

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
      const test = await waitForMessage(socket);
      const data = decode(test);
      assert.deepStrictEqual(data, {
        type: 2,
        nsp: "/",
        data: ["message-back", "message to main namespace", "test", 1],
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

        const data = decode(await waitForMessage(socket));
        assert.deepStrictEqual(data, {
          type: 2,
          nsp: "/",
          id: 1,
          data: ["emit-with-ack", 1, "2", { 3: [true] }],
        });
        socket.send(
          encode({ type: 3, nsp: "/", id: 1, data: [1, "2", { 3: [true] }] }),
        );

        const data2 = decode(await waitForMessage(socket));
        assert.deepStrictEqual(data2, {
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

        let packet = decode(await waitForMessage(socket));
        assert.deepStrictEqual(packet, {
          type: 2,
          nsp: "/",
          id: 1,
          data: ["emit-with-ack", ...BINS, "test"],
        });

        socket.send(
          encode({ type: 3, nsp: "/", id: 1, data: [...BINS, "test"] }),
        );

        packet = decode(await waitForMessage(socket));
        assert.deepStrictEqual(packet, {
          type: 2,
          nsp: "/",
          data: ["emit-with-ack", ...BINS, "test"],
        });
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

      const data = decode(await waitForMessage(socket));
      assert.deepStrictEqual(data, {
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
      const data = decode(await waitForMessage(socket));
      assert.deepStrictEqual(data, {
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

      const data = decode(await waitForMessage(socket));
      assert.deepStrictEqual(data, {
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

      const data = decode(await waitForMessage(socket));
      assert.deepStrictEqual(data, {
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
