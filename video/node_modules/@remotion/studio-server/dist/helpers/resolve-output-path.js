"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.resolveOutputPath = void 0;
const node_path_1 = __importDefault(require("node:path"));
const resolveOutputPath = (remotionRoot, filePath) => {
    const absolutePath = node_path_1.default.join(remotionRoot, filePath);
    const relativeToRoot = node_path_1.default.relative(remotionRoot, absolutePath);
    if (relativeToRoot.startsWith('..')) {
        throw new Error(`Not allowed to write to ${relativeToRoot}`);
    }
    return absolutePath;
};
exports.resolveOutputPath = resolveOutputPath;
