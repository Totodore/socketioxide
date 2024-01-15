export declare const XHR: any;
export declare function createCookieJar(): CookieJar;
interface Cookie {
    name: string;
    value: string;
    expires?: Date;
}
/**
 * @see https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Set-Cookie
 */
export declare function parse(setCookieString: string): Cookie;
export declare class CookieJar {
    private cookies;
    parseCookies(xhr: any): void;
    addCookies(xhr: any): void;
}
export {};
