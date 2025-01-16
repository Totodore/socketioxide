import { describe, it } from "node:test";
import {
  POLLING_URL,
  WS_URL,
  waitForMessage,
  PING_INTERVAL,
  PING_TIMEOUT,
  waitForEvent,
  sleep,
  waitForPackets,
  WebSocketStream,
} from "./utils.ts";
import assert from "node:assert";

function decodePayload(payload: string) {
  const firstColonIndex = payload.indexOf(":");
  const length = payload.substring(0, firstColonIndex);
  const packet = payload.substring(firstColonIndex + 1);
  return [length, packet];
}

async function initLongPollingSession() {
  const response = await fetch(
    `${POLLING_URL}/socket.io/?EIO=3&transport=polling`,
  );
  const text = await response.text();
  const [, content] = decodePayload(text);
  return JSON.parse(content.substring(1)).sid;
}

async function initSocketIOConnection() {
  const socket = new WebSocketStream(
    `${WS_URL}/socket.io/?EIO=3&transport=websocket`,
  );
  socket.binaryType = "arraybuffer";

  await waitForMessage(socket); // Socket.IO handshake
  await waitForMessage(socket); // Socket.IO / namespace handshake
  await waitForMessage(socket); // auth packet

  return socket;
}

describe("Engine.IO protocol", () => {
  describe("handshake", () => {
    describe("HTTP long-polling", () => {
      it("should successfully open a session", async () => {
        const response = await fetch(
          `${POLLING_URL}/socket.io/?EIO=3&transport=polling`,
        );

        assert.equal(response.status, 200);

        const text = await response.text();
        const [length, content] = decodePayload(text);

        assert.equal(length, content.length.toString());
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
        const response = await fetch(`${POLLING_URL}/socket.io/?EIO=3`);

        assert.equal(response.status, 400);

        const response2 = await fetch(
          `${POLLING_URL}/socket.io/?EIO=3&transport=abc`,
        );

        assert.equal(response2.status, 400);
      });

      it("should fail with an invalid request method", async () => {
        const response = await fetch(
          `${POLLING_URL}/socket.io/?EIO=3&transport=polling`,
          {
            method: "post",
          },
        );

        assert.equal(response.status, 400);

        const response2 = await fetch(
          `${POLLING_URL}/socket.io/?EIO=3&transport=polling`,
          {
            method: "put",
          },
        );

        assert.equal(response2.status, 400);
      });
    });

    describe("WebSocket", () => {
      it("should successfully open a session", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/socket.io/?EIO=3&transport=websocket`,
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
        const socket = new WebSocketStream(`${WS_URL}/socket.io/?EIO=3`);

        socket.on("error", () => {});

        waitForEvent(socket, "close");

        const socket2 = new WebSocketStream(
          `${WS_URL}/socket.io/?EIO=3&transport=abc`,
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
          const pushResponse = await fetch(
            `${POLLING_URL}/socket.io/?EIO=3&transport=polling&sid=${sid}`,
            {
              method: "post",
              body: "1:2",
            },
          );

          assert.equal(pushResponse.status, 200);

          const pollResponse = await fetch(
            `${POLLING_URL}/socket.io/?EIO=3&transport=polling&sid=${sid}`,
          );

          assert.equal(pollResponse.status, 200);

          const pollContent = await pollResponse.text();

          if (i === 0) {
            assert.equal(pollContent, `2:4013:42["auth",{}]1:3`);
          } else {
            assert.equal(pollContent, "1:3");
          }
        }
      });

      it("should close the session upon ping timeout", async () => {
        const sid = await initLongPollingSession();

        await sleep(PING_INTERVAL + PING_TIMEOUT + 100);

        const pollResponse = await fetch(
          `${POLLING_URL}/socket.io/?EIO=3&transport=polling&sid=${sid}`,
        );

        assert.equal(pollResponse.status, 400);
      });
    });

    describe("WebSocket", () => {
      it("should send ping/pong packets", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/socket.io/?EIO=3&transport=websocket`,
        );

        await waitForMessage(socket); // handshake
        await waitForMessage(socket); // connect
        await waitForMessage(socket); // ns auth echo

        for (let i = 0; i < 3; i++) {
          socket.send("2");

          const data = await waitForMessage(socket);

          assert.equal(data, "3");
        }

        socket.close();
      });

      it("should close the session upon ping timeout", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/socket.io/?EIO=3&transport=websocket`,
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
          fetch(`${POLLING_URL}/socket.io/?EIO=3&transport=polling&sid=${sid}`),
          fetch(
            `${POLLING_URL}/socket.io/?EIO=3&transport=polling&sid=${sid}`,
            {
              method: "post",
              body: "1:1",
            },
          ),
        ]);

        assert.equal(pollResponse.status, 200);

        const pullContent = await pollResponse.text();

        assert.equal(pullContent, `2:4013:42["auth",{}]`);

        const pollResponse2 = await fetch(
          `${POLLING_URL}/socket.io/?EIO=3&transport=polling&sid=${sid}`,
        );

        assert.equal(pollResponse2.status, 400);
      });
    });

    describe("WebSocket", () => {
      it("should forcefully close the session", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/socket.io/?EIO=3&transport=websocket`,
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
        `${WS_URL}/socket.io/?EIO=3&transport=websocket&sid=${sid}`,
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
        `${WS_URL}/socket.io/?EIO=3&transport=websocket&sid=${sid}`,
      );

      await waitForEvent(socket, "open");
      socket.send("2probe");
      socket.send("5");

      const pollResponse = await fetch(
        `${POLLING_URL}/socket.io/?EIO=3&transport=polling&sid=${sid}`,
      );

      assert.equal(pollResponse.status, 400);
    });

    it("should ignore WebSocket connection with same sid after upgrade", async () => {
      const sid = await initLongPollingSession();

      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=3&transport=websocket&sid=${sid}`,
      );

      await waitForEvent(socket, "open");
      socket.send("2probe");
      socket.send("5");

      const socket2 = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=3&transport=websocket&sid=${sid}`,
      );

      await waitForEvent(socket2, "close");
    });
  });
});

describe("Socket.IO protocol", () => {
  describe("connect", () => {
    it("should be connected by default to the main namespace", async () => {
      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=3&transport=websocket`,
      );

      await waitForMessage(socket); // Engine.IO handshake

      await waitForMessage(socket); // Socket.IO / namespace handshake
      await waitForMessage(socket); // auth packet

      socket.send('42["message","message to main namespace",1, "test"]');
      const data = await waitForMessage(socket);
      assert.equal(
        data,
        '42["message-back","message to main namespace",1,"test"]',
      );
    });

    it("should allow connection to a custom namespace", async () => {
      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=3&transport=websocket`,
      );

      await waitForMessage(socket); // Engine.IO handshake
      await waitForMessage(socket); // Socket.IO / namespace handshake
      await waitForMessage(socket); // auth packet

      socket.send("40/custom,");

      const data: string = await waitForMessage(socket);

      assert.equal(data.substring(0, 9), "40/custom");
    });

    it("should disallow connection to an unknown namespace", async () => {
      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=3&transport=websocket`,
      );

      await waitForMessage(socket); // Engine.IO handshake
      await waitForMessage(socket); // Socket.IO / namespace handshake
      await waitForMessage(socket); // auth packet

      socket.send("40/random");

      const data = await waitForMessage(socket);

      assert.equal(data, '44/random,{"message":"Invalid namespace"}');
    });

    it("should disallow connection with an invalid handshake", async () => {
      const socket = new WebSocketStream(
        `${WS_URL}/socket.io/?EIO=3&transport=websocket`,
      );

      await waitForMessage(socket); // Engine.IO handshake
      await waitForMessage(socket); // Socket.IO / namespace handshake
      await waitForMessage(socket); // auth packet

      socket.send("4abc");

      await waitForEvent(socket, "close");
    });
  });

  describe("disconnect", () => {
    it("should disconnect from the main namespace", async () => {
      const socket = await initSocketIOConnection();

      socket.send("41");

      await waitForEvent(socket, "close");
    });

    it("should connect then disconnect from a custom namespace", async () => {
      const socket = await initSocketIOConnection();

      socket.send("40/custom");

      await waitForMessage(socket); // Socket.IO handshake
      await waitForMessage(socket); // auth packet

      socket.send("41/custom");
      socket.send('42["message","message to main namespace",1,"test"]');

      const data = await waitForMessage(socket);

      assert.equal(
        data,
        '42["message-back","message to main namespace",1,"test"]',
      );
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
      socket.send(Uint8Array.from([4, 1, 2, 3]));
      socket.send(Uint8Array.from([4, 4, 5, 6]));

      let packets = await waitForPackets(socket, 3);
      assert.deepStrictEqual(packets[0], `452-1["emit-with-ack",${DATA}]`);
      assert.deepStrictEqual(packets[1], Uint8Array.from([4, 1, 2, 3]).buffer);
      assert.deepStrictEqual(packets[2], Uint8Array.from([4, 4, 5, 6]).buffer);

      socket.send(`462-1[${DATA}]`);
      socket.send(Uint8Array.from([4, 1, 2, 3]));
      socket.send(Uint8Array.from([4, 4, 5, 6]));

      packets = await waitForPackets(socket, 3);
      assert.deepStrictEqual(packets[0], `452-["emit-with-ack",${DATA}]`);
      assert.deepStrictEqual(packets[1], Uint8Array.from([4, 1, 2, 3]).buffer);
      assert.deepStrictEqual(packets[2], Uint8Array.from([4, 4, 5, 6]).buffer);
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
        '452-["message",{"_placeholder":true,"num":0},{"_placeholder":true,"num":1},"test"]',
      );
      socket.send(Uint8Array.from([4, 1, 2, 3]));
      socket.send(Uint8Array.from([4, 4, 5, 6]));

      const packets = await waitForPackets(socket, 3);

      assert.deepStrictEqual(
        packets[0],
        '452-["message-back",{"_placeholder":true,"num":0},{"_placeholder":true,"num":1},"test"]',
      );
      assert.deepStrictEqual(packets[1], Uint8Array.from([4, 1, 2, 3]).buffer);
      assert.deepStrictEqual(packets[2], Uint8Array.from([4, 4, 5, 6]).buffer);

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
        '452-789["message-with-ack",{"_placeholder":true,"num":0},{"_placeholder":true,"num":1},"test"]',
      );
      socket.send(Uint8Array.from([4, 1, 2, 3]));
      socket.send(Uint8Array.from([4, 4, 5, 6]));

      const packets = await waitForPackets(socket, 3);

      assert.deepStrictEqual(
        packets[0],
        '462-789[{"_placeholder":true,"num":0},{"_placeholder":true,"num":1},"test"]',
      );
      assert.deepStrictEqual(packets[1], Uint8Array.from([4, 1, 2, 3]).buffer);
      assert.deepStrictEqual(packets[2], Uint8Array.from([4, 4, 5, 6]).buffer);

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
