"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getFolderId = void 0;
const getFolderId = ({ folderName, parentName, }) => {
    return [parentName, folderName].filter(Boolean).join('/');
};
exports.getFolderId = getFolderId;
