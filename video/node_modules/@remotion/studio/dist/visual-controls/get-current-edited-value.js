"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getVisualControlEditedValue = void 0;
const getVisualControlEditedValue = ({ handles, key, }) => {
    const handle = handles === null || handles === void 0 ? void 0 : handles[key];
    if (handle === undefined) {
        return null;
    }
    return handle.unsavedValue;
};
exports.getVisualControlEditedValue = getVisualControlEditedValue;
