import { spawn_servers, spawn_sockets, TEST, timeout } from "./fixture.ts";
import assert from "assert";

assert(!!process.env.CMD, "CMD env var must be set");

// * Spawn 10 sockets on 3 servers
// * Call a `broadcast` event on each socket
// * Expect the socket to broadcast a message to all other sockets
async function broadcast() {
  const sockets = await spawn_sockets([3000, 3001, 3002], 10);
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
    await timeout(prom);
    assert.equal(Object.values(msgs).length, sockets.length);
    for (const msg of msgs) {
      assert.deepStrictEqual(msg, `hello from ${socket.id}`);
    }
  }
  return sockets;
}

async function broadcastWithAck() {
  const sockets = await spawn_sockets([3000, 3001, 3002], 10);
  const expected = sockets.map((s) => `ack from ${s.id}`).sort();
  for (const socket of sockets) {
    socket.on("broadcast_with_ack", (_data, ack) => {
      ack(`ack from ${socket.id}`);
    });
  }
  for (const socket of sockets) {
    const res: string[] = await timeout(
      socket.emitWithAck("broadcast_with_ack"),
    );
    assert.deepStrictEqual(res.sort(), expected);
  }
  return sockets;
}

async function disconnectSocket() {
  const sockets = await spawn_sockets([3000, 3001, 3002], 10);
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
  await timeout(prom);
  for (const socket of sockets) {
    assert(!socket.connected);
  }
}

async function rooms() {
  const sockets = await spawn_sockets([3000, 3001, 3002], 10);
  const expected = [
    "room1",
    "room2",
    "room4",
    "room5",
    ...sockets.map((s) => s.id),
  ].sort();
  for (const socket of sockets) {
    const rooms: string[] = await timeout(socket.emitWithAck("rooms"));
    assert.deepStrictEqual(rooms.sort(), expected);
  }
  return sockets;
}

// * Spawn 10 sockets on 3 servers
// * Call a `fetch_sockets` event on each socket
// * Get the list of sockets and compare it to the expected list
async function fetchSockets() {
  type SocketData = { id: string; ns: string };
  const sockets = await spawn_sockets([3000, 3001, 3002], 10);
  const expected = sockets
    .map((socket) => ({
      id: socket.id,
      ns: "/",
    }))
    .sort((a, b) => a.id!.localeCompare(b.id!));

  for (const socket of sockets) {
    const data: SocketData[] = await timeout(
      socket.emitWithAck("fetch_sockets"),
    );
    const sorted = data
      ?.map((data) => ({ id: data.id, ns: data.ns }))
      ?.sort((a, b) => a.id.localeCompare(b.id));
    assert.deepStrictEqual(sorted, expected);
  }

  return sockets;
}

async function main() {
  await spawn_servers([3000, 3001, 3002]);
  await TEST(broadcast);
  await TEST(broadcastWithAck);
  await TEST(fetchSockets);
  await TEST(disconnectSocket);
  await TEST(rooms);
  process.exit();
}
main();
