import io, { Socket } from "socket.io-client";
import msgpackParser from "socket.io-msgpack-parser";
import assert from "assert";

// Spawn a number of distributed sockets on a list of ports
export async function spawn_sockets(len: number) {
  let sockets: Socket[] = [];
  const parser = process.env.PARSER;
  const ports = process.env.PORTS?.split(",").map(Number);

  for (let i = 0; i < len; i++) {
    const socket = io(`http://localhost:${ports[i % ports.length]}`, {
      parser: parser == "msgpack" ? msgpackParser : null,
    });
    assert(
      await new Promise((resolve) =>
        socket.once("connect", () => resolve(true)),
      ),
    );
    sockets.push(socket);
  }
  return sockets;
}
