import { Transport } from "../transport.js";
import { RawData } from "engine.io-parser";
import { Emitter } from "@socket.io/component-emitter";
export declare class Polling extends Transport {
    private readonly xd;
    private polling;
    private pollXhr;
    private cookieJar?;
    /**
     * XHR Polling constructor.
     *
     * @param {Object} opts
     * @package
     */
    constructor(opts: any);
    get name(): string;
    /**
     * Opens the socket (triggers polling). We write a PING message to determine
     * when the transport is open.
     *
     * @protected
     */
    doOpen(): void;
    /**
     * Pauses polling.
     *
     * @param {Function} onPause - callback upon buffers are flushed and transport is paused
     * @package
     */
    pause(onPause: any): void;
    /**
     * Starts polling cycle.
     *
     * @private
     */
    poll(): void;
    /**
     * Overloads onData to detect payloads.
     *
     * @protected
     */
    onData(data: any): void;
    /**
     * For polling, send a close packet.
     *
     * @protected
     */
    doClose(): void;
    /**
     * Writes a packets payload.
     *
     * @param {Array} packets - data packets
     * @protected
     */
    write(packets: any): void;
    /**
     * Generates uri for connection.
     *
     * @private
     */
    private uri;
    /**
     * Creates a request.
     *
     * @param {String} method
     * @private
     */
    request(opts?: {}): Request;
    /**
     * Sends data.
     *
     * @param {String} data to send.
     * @param {Function} called upon flush.
     * @private
     */
    private doWrite;
    /**
     * Starts a poll cycle.
     *
     * @private
     */
    private doPoll;
}
interface RequestReservedEvents {
    success: () => void;
    data: (data: RawData) => void;
    error: (err: number | Error, context: unknown) => void;
}
export declare class Request extends Emitter<{}, {}, RequestReservedEvents> {
    private readonly opts;
    private readonly method;
    private readonly uri;
    private readonly data;
    private xhr;
    private setTimeoutFn;
    private index;
    static requestsCount: number;
    static requests: {};
    /**
     * Request constructor
     *
     * @param {Object} options
     * @package
     */
    constructor(uri: any, opts: any);
    /**
     * Creates the XHR object and sends the request.
     *
     * @private
     */
    private create;
    /**
     * Called upon error.
     *
     * @private
     */
    private onError;
    /**
     * Cleans up house.
     *
     * @private
     */
    private cleanup;
    /**
     * Called upon load.
     *
     * @private
     */
    private onLoad;
    /**
     * Aborts the request.
     *
     * @package
     */
    abort(): void;
}
export {};
