import {classNames} from "../../../utils/class-names.js";
import {ChevronRightIcon} from "@heroicons/react/20/solid/index.js";
import {rooms} from "../../../utils/rooms.js";

export function Sidebar(props) {
  const {currentRoom, setCurrentRoom} = props;
  
  return (
    <aside className="hidden lg:block lg:w-72 bg-ctp-crust p-4 pr-0">
      <div className="bg-ctp-base p-4 rounded-lg mb-4 h-full overflow-y-scroll">
        <h1 className="text-2xl text-center text-white font-bold mb-4">
          Rooms
        </h1>
        <ul className="mt-4">
          {rooms?.map((room) => (
            <li
              key={room}
              className={classNames(
                "relative flex justify-between gap-x-6 px-4 py-5 hover:bg-ctp-mantle sm:px-6 w-full rounded-md cursor-pointer",
                currentRoom === room ? "bg-ctp-mantle" : "",
              )}
              onClick={() => setCurrentRoom(room)}
            >
              <div className="flex flex-row justify-between w-full align-middle">
                <p className="text-lg font-medium text-ctp-text">{room}</p>
                <ChevronRightIcon className="h-6 w-6 text-ctp-blue"/>
              </div>
            </li>
          ))}
        </ul>
      </div>
    </aside>
  )
}
