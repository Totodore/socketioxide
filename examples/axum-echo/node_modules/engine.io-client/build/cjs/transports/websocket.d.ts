import { Transport } from "../transport.js";
export declare class WS extends Transport {
    private ws;
    /**
     * WebSocket transport constructor.
     *
     * @param {Object} opts - connection options
     * @protected
     */
    constructor(opts: any);
    get name(): string;
    doOpen(): this;
    /**
     * Adds event listeners to the socket
     *
     * @private
     */
    private addEventListeners;
    write(packets: any): void;
    doClose(): void;
    /**
     * Generates uri for connection.
     *
     * @private
     */
    private uri;
    /**
     * Feature detection for WebSocket.
     *
     * @return {Boolean} whether this transport is available.
     * @private
     */
    private check;
}
