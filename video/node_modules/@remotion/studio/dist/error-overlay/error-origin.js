"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.wasErrorLoggedByServer = exports.markErrorMessageAsLoggedByServer = exports.markErrorAsLoggedByServer = void 0;
const errorsLoggedByServer = new WeakSet();
const errorMessagesLoggedByServer = new Set();
const maxLoggedErrorMessages = 100;
const rememberServerLoggedMessage = (message) => {
    if (errorMessagesLoggedByServer.size >= maxLoggedErrorMessages) {
        errorMessagesLoggedByServer.clear();
    }
    errorMessagesLoggedByServer.add(message);
};
const markErrorAsLoggedByServer = (error) => {
    errorsLoggedByServer.add(error);
};
exports.markErrorAsLoggedByServer = markErrorAsLoggedByServer;
const markErrorMessageAsLoggedByServer = (message) => {
    if (message.trim() === '') {
        return;
    }
    rememberServerLoggedMessage(message);
};
exports.markErrorMessageAsLoggedByServer = markErrorMessageAsLoggedByServer;
const wasErrorLoggedByServer = (error) => {
    if (errorsLoggedByServer.has(error)) {
        return true;
    }
    const message = typeof error.message === 'string' ? error.message : null;
    const stack = typeof error.stack === 'string' ? error.stack : null;
    if (message && errorMessagesLoggedByServer.has(message)) {
        return true;
    }
    if (stack) {
        for (const serverLoggedMessage of errorMessagesLoggedByServer) {
            if (stack.includes(serverLoggedMessage)) {
                return true;
            }
        }
    }
    return false;
};
exports.wasErrorLoggedByServer = wasErrorLoggedByServer;
