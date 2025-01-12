import io, { Socket } from "socket.io-client";
import msgpackParser from "socket.io-msgpack-parser";
import { ChildProcess, exec, spawn } from "child_process";
import assert from "assert";
import { open } from "fs/promises";

export async function timeout_recv<T>(
  fn: (resolve: (value: T) => void) => any,
  duration = 500,
) {
  return new Promise<T>((resolve, reject) => {
    fn(resolve);
    setTimeout(() => reject("timeout"), duration);
  });
}
export async function timeout<T>(
  promise: Promise<T>,
  duration = 500,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    setTimeout(() => reject("timeout"), duration);
    promise.then(resolve, reject);
  });
}

export async function spawn_servers(ports: number[]) {
  const args = process.env.CMD!.split(" ");
  const bin = args.shift();
  const servers: [ChildProcess, number][] = [];
  const logs: Record<number, string> = {};
  for (const port of ports) {
    exec(`kill $(lsof -t -i:${port})`);
    console.log("spawning server on port", port);
    const file = (await open(`${port}.log`, "w")).createWriteStream();
    console.log(`EXEC PORT=${port} ${bin} ${args.join(" ")}`);
    const server = spawn(bin, args, {
      shell: true,
      env: {
        ...process.env,
        PORT: port.toString(),
      },
    });
    logs[server.pid] = "";
    server.stdout.pipe(file);
    server.stderr.pipe(file);
  }
  process.on("exit", () => {
    for (const [server, port] of servers) {
      console.log("killing", server.pid);
      server.kill();
      exec(`kill $(lsof -t -i:${port})`);
    }
  });
  process.on("SIGINT", () => process.exit()); // catch ctrl-c
  process.on("SIGTERM", () => process.exit()); // catch kill
  await new Promise((resolve) => setTimeout(resolve, 1000));
  return servers;
}

// Spawn a number of distributed sockets on a list of ports
export async function spawn_sockets(ports: number[], len: number) {
  let sockets: Socket[] = [];
  const parser = process.env.CMD.includes("msgpack") ? msgpackParser : null;
  for (let i = 0; i < len; i++) {
    const socket = io(`http://localhost:${ports[i % ports.length]}`, {
      parser,
    });
    assert(
      await timeout_recv((resolve) =>
        socket.on("connect", () => resolve(true)),
      ),
    );
    sockets.push(socket);
  }
  return sockets;
}
export async function TEST(
  fn: () => Promise<void | Socket[]>,
): Promise<void | Socket[]> {
  console.log(`RUN ${fn.name}`);
  const sockets = await fn();
  if (sockets) {
    for (const socket of sockets) {
      socket.disconnect();
    }
  }
  console.log(`OK ${fn.name}`);
}
