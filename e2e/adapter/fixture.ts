import io, { Socket } from "socket.io-client";
import msgpackParser from "socket.io-msgpack-parser";
import { ChildProcess, spawn } from "child_process";
import assert from "assert";
import { open } from "fs/promises";

export async function kill_servers(servers: ChildProcess[]) {
  for (const server of servers) {
    server.kill();
  }
}

export async function spawn_servers(ports: number[]) {
  const args = process.env.CMD!.split(" ");
  const bin = args.shift()!;
  const servers: ChildProcess[] = [];
  for (const port of ports) {
    const file = (await open(`${port}.log`, "w")).createWriteStream();
    console.log(`EXEC PORT=${port} ${bin} ${args.join(" ")}`);
    const server = spawn(bin, args, {
      detached: false,
      timeout: 10000,
      env: {
        ...process.env,
        PORT: port.toString(),
      },
    });
    server.stdout.pipe(file);
    server.stderr.pipe(file);
    servers.push(server);
  }
  await new Promise((resolve) => setTimeout(resolve, 1000));
  return servers;
}

// Spawn a number of distributed sockets on a list of ports
export async function spawn_sockets(ports: number[], len: number) {
  let sockets: Socket[] = [];
  const parser = process.env.CMD?.includes("msgpack") ? msgpackParser : null;
  for (let i = 0; i < len; i++) {
    const socket = io(`http://localhost:${ports[i % ports.length]}`, {
      parser,
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
