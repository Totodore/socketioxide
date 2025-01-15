import { describe, it } from "node:test";
import {
  WS_URL,
  POLLING_URL,
  PING_INTERVAL,
  PING_TIMEOUT,
  sleep,
  waitForPackets,
  waitForMessage,
  WebSocketStream,
  waitForEvent,
} from "./utils.ts";
import assert from "assert";

async function initLongPollingSession() {
  const response = await fetch(
    `${POLLING_URL}/socket.io/?EIO=4&transport=polling`,
  );
  const content = await response.text();
  return JSON.parse(content.substring(1)).sid;
}

async function initSocketIOConnection() {
  const socket = new WebSocketStream(
    `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
  );
  socket.binaryType = "arraybuffer";

  await waitForMessage(socket); // Engine.IO handshake

  socket.send("40");

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

  describe("heartbeat", { timeout: 5000 }, function () {
    describe("HTTP long-polling", () => {
      it("should send ping/pong packets", async () => {
        const sid = await initLongPollingSession();

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

      socket.send("40");

      const data: string = await waitForMessage(socket);
      assert.equal(data.substring(0, 2), "40");
      const handshake = JSON.parse(data.substring(2));
      assert.deepStrictEqual(Object.keys(handshake), ["sid"]);
      assert.equal(typeof handshake.sid, "string");

      const authPacket = await waitForMessage(socket);

      assert.equal(authPacket, '42["auth",{}]');
    });

    it("should allow connection to the main namespace with a payload", async () => {
      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
      );

      await waitForMessage(socket); // Engine.IO handshake

      socket.send('40{"token":"123"}');

      const data: string = await waitForMessage(socket);

      assert.equal(data.substring(0, 2), "40");

      const handshake = JSON.parse(data.substring(2));

      assert.deepStrictEqual(Object.keys(handshake), ["sid"]);
      assert.equal(typeof handshake.sid, "string");

      const authPacket = await waitForMessage(socket);

      assert.equal(authPacket, '42["auth",{"token":"123"}]');
    });

    it("should allow connection to a custom namespace", async () => {
      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
      );

      await waitForMessage(socket); // Engine.IO handshake

      socket.send("40/custom,");

      const data: string = await waitForMessage(socket);

      assert.equal(data.substring(0, 2), "40");

      const handshake = JSON.parse(data.substring(10));

      assert.deepStrictEqual(Object.keys(handshake), ["sid"]);
      assert.equal(typeof handshake.sid, "string");

      const authPacket = await waitForMessage(socket);

      assert.equal(authPacket, '42/custom,["auth",{}]');
    });

    it("should allow connection to a custom namespace with a payload", async () => {
      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
      );

      await waitForMessage(socket); // Engine.IO handshake

      socket.send('40/custom,{"token":"abc"}');

      const data: string = await waitForMessage(socket);

      assert.equal(data.substring(0, 10), "40/custom,");

      const handshake = JSON.parse(data.substring(10));

      assert.deepStrictEqual(Object.keys(handshake), ["sid"]);
      assert.equal(typeof handshake.sid, "string");

      const authPacket = await waitForMessage(socket);

      assert.equal(authPacket, '42/custom,["auth",{"token":"abc"}]');
    });

    it("should disallow connection to an unknown namespace", async () => {
      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
      );

      await waitForMessage(socket); // Engine.IO handshake

      socket.send("40/random");

      const data = await waitForMessage(socket);

      assert.equal(data, '44/random,{"message":"Invalid namespace"}');
    });

    it("should disallow connection with an invalid handshake", async () => {
      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=4&transport=websocket`,
      );

      await waitForMessage(socket); // Engine.IO handshake

      socket.send("4abc");

      await waitForEvent(socket, "close");
    });
  });

  describe("disconnect", () => {
    it("should disconnect from the main namespace", async () => {
      const socket = await initSocketIOConnection();

      socket.send("41");

      const data = await waitForMessage(socket);

      assert.equal(data, "2");
    });

    it("should connect then disconnect from a custom namespace", async () => {
      const socket = await initSocketIOConnection();

      await waitForMessage(socket); // ping

      socket.send("40/custom");

      await waitForMessage(socket); // Socket.IO handshake
      await waitForMessage(socket); // auth packet

      socket.send("41/custom");
      socket.send('42["message","message to main namespace",1,2]');

      const data = await waitForMessage(socket);

      assert.equal(data, '42["message-back","message to main namespace",1,2]');
    });
  });

  describe("acknowledgements", () => {
    it("should emit with an ack expectation", async () => {
      const socket = await initSocketIOConnection();

      socket.send('42["emit-with-ack",1,"2",{"3":[true]}]');

      const data = await waitForMessage(socket);
      assert.equal(data, '421["emit-with-ack",1,"2",{"3":[true]}]');
      socket.send('431[1,"2",{"3":[true]}]');

      const data2 = await waitForMessage(socket);
      assert.equal(data2, '42["emit-with-ack",1,"2",{"3":[true]}]');
    });

    it("should emit with a binary ack expectation", async () => {
      const socket = await initSocketIOConnection();
      const DATA =
        '{"_placeholder":true,"num":0},{"_placeholder":true,"num":1},"test"';

      socket.send(`452-["emit-with-ack",${DATA}]`);
      socket.send(Uint8Array.from([1, 2, 3]));
      socket.send(Uint8Array.from([4, 5, 6]));

      let packets = await waitForPackets(socket, 3);
      assert.deepStrictEqual(packets[0], `452-1["emit-with-ack",${DATA}]`);
      assert.deepStrictEqual(packets[1], Uint8Array.from([1, 2, 3]).buffer);
      assert.deepStrictEqual(packets[2], Uint8Array.from([4, 5, 6]).buffer);

      socket.send(`462-1[${DATA}]`);
      socket.send(Uint8Array.from([1, 2, 3]));
      socket.send(Uint8Array.from([4, 5, 6]));

      packets = await waitForPackets(socket, 3);
      assert.deepStrictEqual(packets[0], `452-["emit-with-ack",${DATA}]`);
      assert.deepStrictEqual(packets[1], Uint8Array.from([1, 2, 3]).buffer);
      assert.deepStrictEqual(packets[2], Uint8Array.from([4, 5, 6]).buffer);
    });
  });

  describe("message", () => {
    it("should send a plain-text packet", async () => {
      const socket = await initSocketIOConnection();

      socket.send('42["message",1,"2",{"3":[true]}]');

      const data = await waitForMessage(socket);

      assert.equal(data, '42["message-back",1,"2",{"3":[true]}]');
    });

    it("should send a packet with binary attachments", async () => {
      const socket = await initSocketIOConnection();

      socket.send(
        '452-["message",1,{"_placeholder":true,"num":0},{"_placeholder":true,"num":1}]',
      );
      socket.send(Uint8Array.from([1, 2, 3]));
      socket.send(Uint8Array.from([4, 5, 6]));

      const packets = await waitForPackets(socket, 3);

      assert.deepStrictEqual(
        packets[0],
        '452-["message-back",1,{"_placeholder":true,"num":0},{"_placeholder":true,"num":1}]',
      );
      assert.deepStrictEqual(packets[1], Uint8Array.from([1, 2, 3]).buffer);
      assert.deepStrictEqual(packets[2], Uint8Array.from([4, 5, 6]).buffer);

      socket.close();
    });

    it("should send a plain-text packet with an ack", async () => {
      const socket = await initSocketIOConnection();

      socket.send('42456["message-with-ack",1,"2",{"3":[false]}]');

      const data = await waitForMessage(socket);

      assert.equal(data, '43456[1,"2",{"3":[false]}]');
    });

    it("should send a packet with binary attachments and an ack", async () => {
      const socket = await initSocketIOConnection();

      socket.send(
        '452-789["message-with-ack",1,{"_placeholder":true,"num":0},{"_placeholder":true,"num":1}]',
      );
      socket.send(Uint8Array.from([1, 2, 3]));
      socket.send(Uint8Array.from([4, 5, 6]));

      const packets = await waitForPackets(socket, 3);

      assert.deepStrictEqual(
        packets[0],
        '462-789[1,{"_placeholder":true,"num":0},{"_placeholder":true,"num":1}]',
      );
      assert.deepStrictEqual(packets[1], Uint8Array.from([1, 2, 3]).buffer);
      assert.deepStrictEqual(packets[2], Uint8Array.from([4, 5, 6]).buffer);

      socket.close();
    });

    it("should close the connection upon invalid format (unknown packet type)", async () => {
      const socket = await initSocketIOConnection();

      socket.send("4abc");

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
