import { spawn_sockets } from "./fixture.ts";
import assert from "assert";
import { describe, it, beforeEach, afterEach } from "node:test";
import { Socket } from "socket.io-client";

assert(process.env.PORTS != null, "PORTS env var must be set");
assert(process.env.PARSER != null, "PARSER env var must be set");

describe("adapter tests", { timeout: 60000 }, () => {
  beforeEach(async (ctx) => {
    const sockets = await spawn_sockets(10);
    (ctx as any).sockets = sockets;
  });

  afterEach(async (ctx) => {
    const sockets: Socket[] = (ctx as any).sockets ?? [];
    for (const socket of sockets) {
      socket.disconnect();
    }
  });

  describe("broadcast tests", () => {
    it("should broadcast a packet sent from a socket to all other sockets", async (ctx) => {
      const sockets: Socket[] = (ctx as any).sockets;
      for (const socket of sockets) {
        let msgs: string[] = [];
        const prom = new Promise((resolve) => {
          for (const socket of sockets) {
            socket.once("broadcast", (data: string) => {
              msgs.push(data);
              if (msgs.length === sockets.length) resolve(null);
            });
          }
        });

        socket.emit("broadcast");
        await prom;
        assert.equal(Object.values(msgs).length, sockets.length);
        for (const msg of msgs) {
          assert.deepStrictEqual(msg, `hello from ${socket.id}`);
        }
      }
    });

    it("should broadcast a packet sent from a socket to all other sockets and get an ack from each socket", async (ctx) => {
      const sockets: Socket[] = (ctx as any).sockets;
      const expected = sockets.map((s) => `ack from ${s.id}`).sort();
      for (const socket of sockets) {
        socket.on("broadcast_with_ack", (_data, ack) => {
          ack(`ack from ${socket.id}`);
        });
      }
      for (const socket of sockets) {
        const res: string[] = await socket.emitWithAck("broadcast_with_ack");
        assert.deepStrictEqual(res.sort(), expected);
      }
    });

    it("should disconnect all sockets from a given one", async (ctx) => {
      const sockets: Socket[] = (ctx as any).sockets;
      let cnt = 0;
      const prom = new Promise((resolve) => {
        for (const socket of sockets) {
          socket.on("disconnect", () => {
            cnt++;
            if (cnt === sockets.length) resolve(null);
          });
        }
      });
      sockets[0].emit("disconnect_socket");
      await prom;
      for (const socket of sockets) {
        assert(!socket.connected);
      }
    });

    it("should fetch all socket rooms from one socket", async (ctx) => {
      const sockets: Socket[] = (ctx as any).sockets;
      const expected = [
        "room1",
        "room2",
        "room4",
        "room5",
        ...sockets.map((s) => s.id),
      ].sort();
      for (const socket of sockets) {
        const rooms: string[] = await socket.emitWithAck("rooms");
        assert.deepStrictEqual(rooms.sort(), expected);
      }
    });

    it("should fetch all sockets from one socket", async (ctx) => {
      type SocketData = { id: string; ns: string };
      const sockets: Socket[] = (ctx as any).sockets;
      const expected = sockets
        .map((socket) => ({
          id: socket.id,
          ns: "/",
        }))
        .sort((a, b) => a.id!.localeCompare(b.id!));

      for (const socket of sockets) {
        const data: SocketData[] = await socket.emitWithAck("fetch_sockets");
        const sorted = data
          ?.map((data) => ({ id: data.id, ns: data.ns }))
          ?.sort((a, b) => a.id.localeCompare(b.id));
        assert.deepStrictEqual(sorted, expected);
      }
    });

    it("should make all sockets join a room from one socket", async (ctx) => {
      const sockets: Socket[] = (ctx as any).sockets;
      const expected = [
        "room1",
        "room2",
        "room4",
        "room5",
        ...sockets.map((s) => s.id),
      ];
      await sockets[0].emitWithAck("join_room");
      const rooms: string[] = await sockets[0].emitWithAck("rooms");
      assert.deepStrictEqual(rooms.sort(), [...expected, "room7"].sort());
    });
    it("should make all sockets leave a room from one socket", async (ctx) => {
      const sockets: Socket[] = (ctx as any).sockets;
      const expected = [
        "room1",
        "room2",
        "room5",
        ...sockets.map((s) => s.id),
      ].sort();

      await sockets[0].emitWithAck("leave_room");
      const rooms: string[] = await sockets[0].emitWithAck("rooms");
      assert.deepStrictEqual(rooms.sort(), expected);
    });
  });
  describe("remote sockets actions", () => {
    it("should emit from a remote socket", async (ctx) => {
      const sockets: Socket[] = (ctx as any).sockets;

      for (let i = 0; i < sockets.length; i++) {
        const socket_emit = sockets[i];
        const socket_rcv = sockets[(i + 1) % sockets.length];
        const prom = new Promise((resolve) => socket_rcv.once("emit", resolve));
        await socket_emit.emitWithAck("emit_from_remote_sock", socket_rcv.id);
        assert.equal(await prom, `hello from ${socket_emit.id}`);
      }
    });

    it("should emit with ack from a remote socket", async (ctx) => {
      const sockets: Socket[] = (ctx as any).sockets;

      for (let i = 0; i < sockets.length; i++) {
        const socket_emit = sockets[i];
        const socket_rcv = sockets[(i + 1) % sockets.length];
        const prom = new Promise<[string, (ack: any) => void]>((resolve) =>
          socket_rcv.once("emit", (msg, ack) => resolve([msg, ack])),
        );
        const ack = socket_emit.emitWithAck(
          "emit_with_ack_from_remote_sock",
          socket_rcv.id,
        );
        const [msg, ack_sender] = await prom;
        assert.equal(msg, `hello from ${socket_emit.id}`);
        ack_sender(`hi from ${socket_rcv.id}`);
        assert.equal(await ack, `hi from ${socket_rcv.id}`);
      }
    });

    it("should join a room from a remote socket", async (ctx) => {
      const sockets: Socket[] = (ctx as any).sockets;

      for (let i = 0; i < sockets.length; i++) {
        const socket_emit = sockets[i];
        const socket_rcv = sockets[(i + 1) % sockets.length];
        await socket_emit.emitWithAck(
          "join_room_from_remote_sock",
          socket_rcv.id,
        );
        const rooms = await socket_emit.emitWithAck(
          "get_rooms_remote_sock",
          socket_rcv.id,
        );
        assert(rooms.includes(`hello from ${socket_emit.id}`));
      }
    });

    it("should leave a room from a remote socket", async (ctx) => {
      const sockets: Socket[] = (ctx as any).sockets;

      for (let i = 0; i < sockets.length; i++) {
        const socket_emit = sockets[i];
        const socket_rcv = sockets[(i + 1) % sockets.length];
        const rooms_before = await socket_emit.emitWithAck(
          "get_rooms_remote_sock",
          socket_rcv.id,
        );
        assert(rooms_before.includes("room4"));

        await socket_emit.emitWithAck(
          "leave_room_from_remote_sock",
          socket_rcv.id,
        );
        const rooms = await socket_emit.emitWithAck(
          "get_rooms_remote_sock",
          socket_rcv.id,
        );
        assert(!rooms.includes(`room4`));
      }
    });

    it("should disconnect a remote socket", async (ctx) => {
      const sockets: Socket[] = (ctx as any).sockets;

      for (let i = 1; i < sockets.length; i++) {
        const socket_emit = sockets[i];
        const socket_rcv = sockets[i - 1];
        const prom = new Promise((resolve) =>
          socket_rcv.once("disconnect", resolve),
        );
        await socket_emit.emitWithAck("disconnect_remote_sock", socket_rcv.id);
        await prom;
      }
    });

    it("should get rooms from remote socket", async (ctx) => {
      const sockets: Socket[] = (ctx as any).sockets;
      const ROOMS = ["room1", "room2", "room4", "room5"];
      for (let i = 0; i < sockets.length; i++) {
        const socket_emit = sockets[i];
        const socket_rcv = sockets[(i + 1) % sockets.length];
        const rooms = await socket_emit.emitWithAck(
          "get_rooms_remote_sock",
          socket_rcv.id,
        );
        assert.deepEqual(rooms.sort(), [...ROOMS, socket_rcv.id].sort());
      }
    });
  });
});
