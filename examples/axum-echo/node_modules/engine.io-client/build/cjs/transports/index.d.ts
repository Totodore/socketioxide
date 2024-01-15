import { Polling } from "./polling.js";
import { WS } from "./websocket.js";
import { WT } from "./webtransport.js";
export declare const transports: {
    websocket: typeof WS;
    webtransport: typeof WT;
    polling: typeof Polling;
};
