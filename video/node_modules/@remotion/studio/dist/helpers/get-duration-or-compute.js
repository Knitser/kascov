"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getDurationOrCompute = void 0;
const getDurationOrCompute = async (input) => {
    var _a;
    return ((_a = (await input.getDurationFromMetadata(undefined, {
        skipLiveWait: true,
    }))) !== null && _a !== void 0 ? _a : input.computeDuration(undefined, { skipLiveWait: true }));
};
exports.getDurationOrCompute = getDurationOrCompute;
