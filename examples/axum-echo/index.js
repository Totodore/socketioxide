import io from "socket.io-client";
import parser from "./parser.js";

const enc = new TextEncoder();
const socket = io(`ws://127.0.0.1:3000/`, {
    // parser: parser
});

socket.on("connect", () => {
    socket.emit("message-with-ack", { data: "nice" }, enc.encode("Some nice broadcasting yk =)"), (...response) => {
        console.log("\x1b[33msqw:broadcast response:", JSON.stringify(response), "\x1b[0m");
    });
})