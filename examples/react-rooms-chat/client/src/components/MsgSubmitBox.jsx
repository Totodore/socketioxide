import {useState} from "react";

export function MsgSubmitBox(props) {
  const {socket, currentRoom} = props;
  const [input, setInput] = useState("");
  
  const sendMessage = (e) => {
    e.preventDefault();
    socket?.emit("message", {
      text: input,
      room: currentRoom,
    });
    setInput("");
  };
  
  return (
    <form className="flex h-11" onSubmit={sendMessage}>
      <input
        type="text"
        value={input}
        onChange={(e) => setInput(e.target.value)}
        className="flex-1 p-2 rounded-l-md bg-ctp-text text-ctp-base placeholder-ctp-subtext0"
        placeholder="Enter something englightened..."
      />
      <button
        type="submit"
        className="bg-ctp-blue px-6 font-bold text-ctp-base p-2 rounded-r-md"
      >
        Send
      </button>
    </form>
  )
}
