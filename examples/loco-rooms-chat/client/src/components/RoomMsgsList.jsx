import {classNames} from "../utils/class-names.js";
import {colorForName} from "../utils/color-for-name.js";

export function RoomMsgsList(props) {
  const {messages} = props
  
  return (
    <ul className="p-4">
      {messages?.map((msg, index) => (
        <li
          key={index}
          className="flex w-full justify-start gap-x-4 mb-4 align-top"
        >
          <div>
            <div className="flex flex-row gap-x-6 items-center">
              <p
                className={classNames(
                  "text-sm font-semibold",
                  `text-${colorForName(msg.user)}`,
                )}
              >
                {msg.user}
              </p>
              <p className="text-ctp-text text-sm">
                {msg.date.toLocaleString()}
              </p>
            </div>
            <p className="text-ctp-text mt-1 text-lg">{msg.text}</p>
          </div>
        </li>
      ))}
    </ul>
  )
}
