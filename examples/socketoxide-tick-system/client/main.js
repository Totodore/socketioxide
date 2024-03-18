const { io } = require("socket.io-client");

let sock = io("http://localhost:3000", {
	auth: { token: "test" },
});

sock.on("connect", () => {
	console.log("connected");
});

sock.on("announcer", data => {
	console.log(data);
})

sock.on("disconnect", () => {
	console.log("disconnected");
});

sock.on("error", (err) => {
	console.error(err);
});