import {ChevronRightIcon} from "@heroicons/react/20/solid/index.js";
import {classNames} from "../../../utils/class-names.js";
import {rooms} from "../../../utils/rooms.js";

export function TransitiveSidebarRoomsList(props) {
  const {currentRoom, setCurrentRoom} = props
  return (
    <nav className="flex flex-1 flex-col">
      <ul role="list" className="flex flex-1 flex-col gap-y-7">
        <li>
          <ul role="list" className="-mx-2 space-y-1">
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
                  <p className="text-lg font-medium text-ctp-text">
                    {room}
                  </p>
                  <ChevronRightIcon className="h-6 w-6 text-ctp-blue"/>
                </div>
              </li>
            ))}
          </ul>
        </li>
      </ul>
    </nav>
  )
}
