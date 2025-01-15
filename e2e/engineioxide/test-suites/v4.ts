import { describe, it } from "node:test";
import {
  PING_INTERVAL,
  PING_TIMEOUT,
  WebSocketStream,
  WS_URL,
  waitForMessage,
  waitForEvent,
  sleep,
  POLLING_URL,
} from "./utils.ts";
import assert from "node:assert";

async function initLongPollingSession() {
  const response = await fetch(
    `${POLLING_URL}/engine.io/?EIO=4&transport=polling`,
  );
  const content = await response.text();
  return JSON.parse(content.substring(1)).sid;
}

describe("Engine.IO protocol", () => {
  describe("handshake", () => {
    describe("HTTP long-polling", () => {
      it("successfully opens a session", async () => {
        const response = await fetch(
          `${POLLING_URL}/engine.io/?EIO=4&transport=polling`,
        );

        assert.deepStrictEqual(response.status, 200);

        const content = await response.text();

        assert.deepStrictEqual(content[0], "0");

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

      it("fails with an invalid 'EIO' query parameter", async () => {
        const response = await fetch(
          `${POLLING_URL}/engine.io/?transport=polling`,
        );

        assert.deepStrictEqual(response.status, 400);

        const response2 = await fetch(
          `${POLLING_URL}/engine.io/?EIO=abc&transport=polling`,
        );

        assert.deepStrictEqual(response2.status, 400);
      });

      it("fails with an invalid 'transport' query parameter", async () => {
        const response = await fetch(`${POLLING_URL}/engine.io/?EIO=4`);

        assert.deepStrictEqual(response.status, 400);

        const response2 = await fetch(
          `${POLLING_URL}/engine.io/?EIO=4&transport=abc`,
        );

        assert.deepStrictEqual(response2.status, 400);
      });

      it("fails with an invalid request method", async () => {
        const response = await fetch(
          `${POLLING_URL}/engine.io/?EIO=4&transport=polling`,
          {
            method: "post",
          },
        );

        assert.deepStrictEqual(response.status, 400);

        const response2 = await fetch(
          `${POLLING_URL}/engine.io/?EIO=4&transport=polling`,
          {
            method: "put",
          },
        );

        assert.deepStrictEqual(response2.status, 400);
      });
    });

    describe("WebSocket", () => {
      it("successfully opens a session", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/engine.io/?EIO=4&transport=websocket`,
        );

        const data: string = await waitForMessage(socket);

        assert.deepStrictEqual(data[0], "0");

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

      it("fails with an invalid 'EIO' query parameter", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/engine.io/?transport=websocket`,
        );

        socket.on("error", () => {});
        waitForEvent(socket, "close");

        const socket2 = new WebSocketStream(
          `${WS_URL}/engine.io/?EIO=abc&transport=websocket`,
        );

        socket2.on("error", () => {});

        waitForEvent(socket2, "close");
      });

      it("fails with an invalid 'transport' query parameter", async () => {
        const socket = new WebSocketStream(`${WS_URL}/engine.io/?EIO=4`);

        socket.on("error", () => {});

        waitForEvent(socket, "close");

        const socket2 = new WebSocketStream(
          `${WS_URL}/engine.io/?EIO=4&transport=abc`,
        );

        socket2.on("error", () => {});

        waitForEvent(socket2, "close");
      });
    });
  });

  describe("message", () => {
    describe("HTTP long-polling", () => {
      it("sends and receives a payload containing one plain text packet", async () => {
        const sid = await initLongPollingSession();

        const pushResponse = await fetch(
          `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
          {
            method: "post",
            body: "4hello",
          },
        );

        assert.deepStrictEqual(pushResponse.status, 200);

        const postContent = await pushResponse.text();

        assert.deepStrictEqual(postContent, "ok");

        const pollResponse = await fetch(
          `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
        );

        assert.deepStrictEqual(pollResponse.status, 200);

        const pollContent = await pollResponse.text();

        assert.deepStrictEqual(pollContent, "4hello");
      });

      it("sends and receives a payload containing several plain text packets", async () => {
        const sid = await initLongPollingSession();

        const pushResponse = await fetch(
          `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
          {
            method: "post",
            body: "4test1\x1e4test2\x1e4test3",
          },
        );

        assert.deepStrictEqual(pushResponse.status, 200);

        const postContent = await pushResponse.text();

        assert.deepStrictEqual(postContent, "ok");

        const pollResponse = await fetch(
          `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
        );

        assert.deepStrictEqual(pollResponse.status, 200);

        const pollContent = await pollResponse.text();

        assert.deepStrictEqual(pollContent, "4test1\x1e4test2\x1e4test3");
      });

      it("sends and receives a payload containing plain text and binary packets", async () => {
        const sid = await initLongPollingSession();

        const pushResponse = await fetch(
          `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
          {
            method: "post",
            body: "4hello\x1ebAQIDBA==",
          },
        );

        assert.deepStrictEqual(pushResponse.status, 200);

        const postContent = await pushResponse.text();

        assert.deepStrictEqual(postContent, "ok");

        const pollResponse = await fetch(
          `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
        );

        assert.deepStrictEqual(pollResponse.status, 200);

        const pollContent = await pollResponse.text();

        assert.deepStrictEqual(pollContent, "4hello\x1ebAQIDBA==");
      });

      it("closes the session upon invalid packet format", async () => {
        const sid = await initLongPollingSession();

        try {
          const pushResponse = await fetch(
            `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
            {
              method: "post",
              body: "abc",
            },
          );

          assert.deepStrictEqual(pushResponse.status, 400);
        } catch (e) {
          // node-fetch throws when the request is closed abnormally
        }

        const pollResponse = await fetch(
          `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
        );

        assert.deepStrictEqual(pollResponse.status, 400);
      });

      it("closes the session upon duplicate poll requests", async () => {
        const sid = await initLongPollingSession();

        const pollResponses = await Promise.all([
          fetch(`${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`),
          sleep(5).then(() =>
            fetch(
              `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}&t=burst`,
            ),
          ),
        ]);

        assert.deepStrictEqual(pollResponses[0].status, 200);

        const content = await pollResponses[0].text();

        assert.deepStrictEqual(content, "1");

        // the Node.js implementation uses HTTP 500 (Internal Server Error), but HTTP 400 seems more suitable
        assert([400, 500].includes(pollResponses[1].status));

        const pollResponse = await fetch(
          `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
        );

        assert.deepStrictEqual(pollResponse.status, 400);
      });
    });

    describe("WebSocket", () => {
      it("sends and receives a plain text packet", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/engine.io/?EIO=4&transport=websocket`,
        );

        await waitForEvent(socket, "open");

        await waitForMessage(socket); // handshake

        socket.send("4hello");

        const data = await waitForMessage(socket);

        assert.deepStrictEqual(data, "4hello");

        socket.close();
      });

      it("sends and receives a binary packet", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/engine.io/?EIO=4&transport=websocket`,
        );
        socket.binaryType = "arraybuffer";

        await waitForMessage(socket); // handshake

        socket.send(Uint8Array.from([1, 2, 3, 4]));

        const data = await waitForMessage(socket);

        assert.deepStrictEqual(data, Uint8Array.from([1, 2, 3, 4]).buffer);

        socket.close();
      });

      it("closes the session upon invalid packet format", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/engine.io/?EIO=4&transport=websocket`,
        );

        await waitForMessage(socket); // handshake

        socket.send("abc");

        await waitForEvent(socket, "close");

        socket.close();
      });
    });
  });

  describe("heartbeat", { timeout: 5000 }, function () {
    describe("HTTP long-polling", () => {
      it("sends ping/pong packets", async () => {
        const sid = await initLongPollingSession();

        for (let i = 0; i < 3; i++) {
          const pollResponse = await fetch(
            `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
          );

          assert.deepStrictEqual(pollResponse.status, 200);

          const pollContent = await pollResponse.text();

          assert.deepStrictEqual(pollContent, "2");

          const pushResponse = await fetch(
            `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
            {
              method: "post",
              body: "3",
            },
          );

          assert.deepStrictEqual(pushResponse.status, 200);
        }
      });

      it("closes the session upon ping timeout", async () => {
        const sid = await initLongPollingSession();

        await sleep(PING_INTERVAL + PING_TIMEOUT);

        const pollResponse = await fetch(
          `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
        );

        assert(
          pollResponse.status == 400 ||
            (pollResponse.status == 200 && (await pollResponse.text()) == "1"),
        );
      });
    });

    describe("WebSocket", () => {
      it("sends ping/pong packets", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/engine.io/?EIO=4&transport=websocket`,
        );

        await waitForMessage(socket); // handshake

        for (let i = 0; i < 3; i++) {
          const data = await waitForMessage(socket);

          assert.deepStrictEqual(data, "2");

          socket.send("3");
        }

        socket.close();
      });

      it("closes the session upon ping timeout", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/engine.io/?EIO=4&transport=websocket`,
        );

        await waitForEvent(socket, "close"); // handshake
      });
    });
  });

  describe("close", () => {
    describe("HTTP long-polling", () => {
      it("forcefully closes the session", async () => {
        const sid = await initLongPollingSession();

        const [pollResponse] = await Promise.all([
          fetch(`${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`),
          fetch(
            `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
            {
              method: "post",
              body: "1",
            },
          ),
        ]);

        assert.deepStrictEqual(pollResponse.status, 200);

        const pullContent = await pollResponse.text();

        assert.deepStrictEqual(pullContent, "6");

        const pollResponse2 = await fetch(
          `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
        );

        assert.deepStrictEqual(pollResponse2.status, 400);
      });
    });

    describe("WebSocket", () => {
      it("forcefully closes the session", async () => {
        const socket = new WebSocketStream(
          `${WS_URL}/engine.io/?EIO=4&transport=websocket`,
        );

        await waitForMessage(socket); // handshake

        socket.send("1");

        await waitForEvent(socket, "close");
      });
    });
  });

  describe("upgrade", () => {
    it("successfully upgrades from HTTP long-polling to WebSocket", async () => {
      const sid = await initLongPollingSession();

      const socket = new WebSocketStream(
        `${WS_URL}/engine.io/?EIO=4&transport=websocket&sid=${sid}`,
      );

      await waitForEvent(socket, "open");

      // send probe
      socket.send("2probe");

      const probeResponse = await waitForMessage(socket);

      assert.deepStrictEqual(probeResponse, "3probe");

      const pollResponse = await fetch(
        `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
      );

      assert.deepStrictEqual(pollResponse.status, 200);

      const pollContent = await pollResponse.text();

      assert.deepStrictEqual(pollContent, "6"); // "noop" packet to cleanly end the HTTP long-polling request

      // complete upgrade
      socket.send("5");

      socket.send("4hello");

      const data = await waitForMessage(socket);

      assert.deepStrictEqual(data, "4hello");
    });

    it("ignores HTTP requests with same sid after upgrade", async () => {
      const sid = await initLongPollingSession();

      const socket = new WebSocketStream(
        `${WS_URL}/engine.io/?EIO=4&transport=websocket&sid=${sid}`,
      );

      await waitForEvent(socket, "open");
      socket.send("2probe");
      socket.send("5");

      const pollResponse = await fetch(
        `${POLLING_URL}/engine.io/?EIO=4&transport=polling&sid=${sid}`,
      );

      assert.deepStrictEqual(pollResponse.status, 400);

      socket.send("4hello");

      await waitForMessage(socket); // 3probe
      const data = await waitForMessage(socket);

      assert.deepStrictEqual(data, "4hello");
    });

    it("ignores WebSocket connection with same sid after upgrade", async () => {
      const sid = await initLongPollingSession();

      const socket = new WebSocketStream(
        `${WS_URL}/engine.io/?EIO=4&transport=websocket&sid=${sid}`,
      );

      await waitForEvent(socket, "open");
      socket.send("2probe");
      socket.send("5");

      const socket2 = new WebSocketStream(
        `${WS_URL}/engine.io/?EIO=4&transport=websocket&sid=${sid}`,
      );

      await waitForEvent(socket2, "close");

      socket.send("4hello");

      await waitForMessage(socket); // 3probe
      const data = await waitForMessage(socket);

      assert.deepStrictEqual(data, "4hello");
    });
  });
});
