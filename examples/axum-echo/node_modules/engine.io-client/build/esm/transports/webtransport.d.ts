import { Transport } from "../transport.js";
import { Packet } from "engine.io-parser";
export declare class WT extends Transport {
    private transport;
    private writer;
    get name(): string;
    protected doOpen(): void;
    protected write(packets: Packet[]): void;
    protected doClose(): void;
}
