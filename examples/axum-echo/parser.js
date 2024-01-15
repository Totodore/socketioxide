import { Encoder as SioEncoder, Decoder as SioDecoder } from "socket.io-parser";

class Encoder extends SioEncoder {
    encode(packet) {
        // const packets = super.encode(packet);
        // console.log("->", packets)
        // return packets;

        return ['51-["baz",{"_placeholder":true,"num":0}]', new Uint8Array([0,1,2,3,4,5,7,8,5,3]).buffer]
    }
}

class Decoder extends SioDecoder {
    add(obj) {
        console.log("<-", obj);
        return super.add(obj);
    }
}

export default { Encoder, Decoder }