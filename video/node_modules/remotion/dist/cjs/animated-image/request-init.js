"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.serializeRequestInit = void 0;
const serializeRequestInit = (requestInit) => {
    if (!requestInit) {
        return null;
    }
    const requestInitWithoutSignal = { ...requestInit };
    delete requestInitWithoutSignal.signal;
    const { headers, ...rest } = requestInitWithoutSignal;
    return JSON.stringify({
        ...rest,
        headers: headers ? Array.from(new Headers(headers).entries()) : null,
    });
};
exports.serializeRequestInit = serializeRequestInit;
