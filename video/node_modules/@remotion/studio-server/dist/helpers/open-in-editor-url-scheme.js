"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.openInEditorViaUrlScheme = void 0;
const node_child_process_1 = __importDefault(require("node:child_process"));
const getVsCodeUrlScheme = (editor) => {
    switch (editor) {
        case 'code':
        case 'Code.exe':
            return 'vscode';
        case 'code-insiders':
        case 'Code - Insiders.exe':
            return 'vscode-insiders';
        case 'vscodium':
        case 'VSCodium.exe':
            return 'vscodium';
        case 'cursor':
        case 'Cursor.exe':
            return 'cursor';
        case 'windsurf':
        case 'Windsurf.exe':
            return 'windsurf';
        default:
            return null;
    }
};
// On macOS, use URL protocol handler for VS Code derivatives.
// This is faster than spawning the CLI which boots a Node.js process.
const openInEditorViaUrlScheme = ({ editor, fileName, lineNumber, colNumber, }) => {
    if (process.platform !== 'darwin') {
        return null;
    }
    const urlScheme = getVsCodeUrlScheme(editor);
    if (!urlScheme) {
        return null;
    }
    const filePath = fileName.startsWith('/') ? fileName.substring(1) : fileName;
    const encodedPath = filePath.split('/').map(encodeURIComponent).join('/');
    const url = `${urlScheme}://file/${encodedPath}:${lineNumber}:${colNumber}`;
    return new Promise((resolve) => {
        const proc = node_child_process_1.default.spawn('open', [url], {
            stdio: 'ignore',
        });
        proc.on('error', () => resolve(false));
        proc.on('close', (code) => resolve(code === 0));
    });
};
exports.openInEditorViaUrlScheme = openInEditorViaUrlScheme;
