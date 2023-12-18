import {useState, useRef, useEffect} from "react";
import "./App.css";
import {io} from "socket.io-client";
import {Bars3Icon, } from "@heroicons/react/24/outline";
import {Sidebar} from "./components/sidebar/normal/Sidebar.jsx";
import {RoomMsgsList} from "./components/RoomMsgsList.jsx";
import {MsgSubmitBox} from "./components/MsgSubmitBox.jsx";
import {rooms} from "./utils/rooms.js";
import {TransitiveSidebar} from "./components/sidebar/transitive/TransitiveSidebar.jsx";

function App() {
  const [messages, setMessages] = useState([]);
  const [currentRoom, setCurrentRoom] = useState(rooms[0]);
  const [socket, setSocket] = useState(null);
  const onceRef = useRef(false);
  const [sidebarOpen, setSidebarOpen] = useState(false);

  useEffect(() => {
    setMessages([]);
    socket?.emit("join", currentRoom);
  }, [currentRoom, socket]);

  useEffect(() => {
    if (onceRef.current) {
      return;
    }

    onceRef.current = true;

    const socket = io("ws://localhost:3000");
    setSocket(socket);

    socket.on("connect", () => {
      console.log("Connected to socket server");
      console.log("joining room", currentRoom);

      socket.emit("join", currentRoom);
    });

    socket.on("message", (msg) => {
      console.log("Message received", msg);
      msg.date = new Date(msg.date);
      setMessages((messages) => [...messages, msg]);
    });

    socket.on("messages", (msgs) => {
      console.log("Messages received", msgs);
      let messages = msgs.messages.map((msg) => {
        msg.date = new Date(msg.date);
        return msg;
      });
      setMessages(messages);
    });
  }, []);

  return (
    <>
      <main className="h-screen w-screen flex text-ctp-text">
        {/*<Transition.Root show={sidebarOpen} as={Fragment}>
          <Dialog
            as="div"
            className="relative z-50 lg:hidden"
            onClose={setSidebarOpen}
          >
            <Transition.Child
              as={Fragment}
              enter="transition-opacity ease-linear duration-300"
              enterFrom="opacity-0"
              enterTo="opacity-100"
              leave="transition-opacity ease-linear duration-300"
              leaveFrom="opacity-100"
              leaveTo="opacity-0"
            >
              <div className="fixed inset-0 bg-gray-900/80"/>
            </Transition.Child>

            <div className="fixed inset-0 flex">
              <Transition.Child
                as={Fragment}
                enter="transition ease-in-out duration-300 transform"
                enterFrom="-translate-x-full"
                enterTo="translate-x-0"
                leave="transition ease-in-out duration-300 transform"
                leaveFrom="translate-x-0"
                leaveTo="-translate-x-full"
              >
                <Dialog.Panel className="relative mr-16 flex w-full max-w-xs flex-1">
                  <Transition.Child
                    as={Fragment}
                    enter="ease-in-out duration-300"
                    enterFrom="opacity-0"
                    enterTo="opacity-100"
                    leave="ease-in-out duration-300"
                    leaveFrom="opacity-100"
                    leaveTo="opacity-0"
                  >
                    <div className="absolute left-full top-0 flex w-16 justify-center pt-5">
                      <button
                        type="button"
                        className="-m-2.5 p-2.5"
                        onClick={() => setSidebarOpen(false)}
                      >
                        <span className="sr-only">Close sidebar</span>
                        <XMarkIcon
                          className="h-6 w-6 text-white"
                          aria-hidden="true"
                        />
                      </button>
                    </div>
                  </Transition.Child>
                  <div className="flex grow flex-col gap-y-5 overflow-y-auto bg-ctp-base px-6 pb-4">
                    <div className="flex h-16 shrink-0 items-center">
                      <h1 className="text-2xl text-white font-bold py-4">
                        Rooms
                      </h1>
                    </div>
                    <TransitiveSidebarRoomsList
                      currentRoom={currentRoom}
                      setCurrentRoom={setCurrentRoom}
                    />
                  </div>
                </Dialog.Panel>
              </Transition.Child>
            </div>
          </Dialog>
        </Transition.Root>*/}
        <TransitiveSidebar
          sidebarOpen={sidebarOpen}
          setSidebarOpen={setSidebarOpen}
          currentRoom={currentRoom}
          setCurrentRoom={setCurrentRoom}
        />
        <Sidebar
          currentRoom={currentRoom}
          setCurrentRoom={setCurrentRoom}
        />
        <div className="h-screen p-4 bg-ctp-crust flex flex-col flex-grow justify-end">
          <div className="bg-ctp-base rounded-t-lg flex-grow">
            <div className="sticky top-0 z-40 flex items-center gap-x-6 bg-ctp-mantle px-2 sm:px-6 lg:hidden">
              <button
                type="button"
                className="-m-2.5 p-2.5 text-gray-400 lg:hidden"
                onClick={() => setSidebarOpen(true)}
              >
                <span className="sr-only">Open sidebar</span>
                <Bars3Icon className="h-6 w-6" aria-hidden="true"/>
              </button>
              <div className="flex-1 text-sm font-semibold leading-6 text-white">
                <h1 className="text-2xl text-white font-bold py-4">
                  {currentRoom}
                </h1>
              </div>
            </div>

            <h1 className="hidden lg:block text-2xl text-center text-white font-bold my-4">
              {currentRoom}
            </h1>
            <RoomMsgsList messages={messages}/>
          </div>
          <MsgSubmitBox socket={socket} currentRoom={currentRoom}/>
        </div>
      </main>
    </>
  );
}

export default App;
