import { io } from "socket.io-client";

const URL = process.env.URL || "http://localhost:3000";
const MAX_CLIENTS = 2000;
const PING_INTERVAL = 1000;
const POLLING_PERCENTAGE = 0.05;

let clientCount = 0;

const createClient = () => {
	// for demonstration purposes, some clients stay stuck in HTTP long-polling
	const transports =
		Math.random() < POLLING_PERCENTAGE ? ["polling"] : ["polling", "websocket"];

	const socket = io(URL, {
		transports,
	});

	setInterval(() => {
		socket.emit("ping");
	}, PING_INTERVAL * 2);

	setInterval(() => {
		socket.emit("ping", Uint8Array.from(Array(1024 * 1024).keys()));
	}, PING_INTERVAL * 2 + PING_INTERVAL);

	socket.on("disconnect", (reason) => {
		console.log(`disconnect due to ${reason}`);
	});

	if (++clientCount < MAX_CLIENTS) {
		socket.on("connect", () => createClient());
	} else {
		console.log("All clients connected stopping in 5 seconds");
		setTimeout(() => {
			process.exit(0);
		}, 5000);
	}
};

createClient();