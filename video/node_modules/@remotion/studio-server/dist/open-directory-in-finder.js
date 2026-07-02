"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.openDirectoryInFinder = void 0;
const node_child_process_1 = require("node:child_process");
const node_os_1 = require("node:os");
const node_path_1 = __importDefault(require("node:path"));
const no_react_1 = require("remotion/no-react");
const openDirectoryInFinder = (dirToOpen, allowedDirectory) => {
    const resolved = node_path_1.default.resolve(allowedDirectory, dirToOpen);
    const relativeToProcessCwd = node_path_1.default.relative(allowedDirectory, resolved);
    if (relativeToProcessCwd.startsWith('..')) {
        throw new Error(`Not allowed to open ${relativeToProcessCwd}`);
    }
    if ((0, node_os_1.platform)() === 'win32') {
        const proc = (0, node_child_process_1.spawn)('explorer.exe', ['/select,', resolved]);
        return new Promise((resolve, reject) => {
            proc.on('exit', (code) => {
                // explorer.exe returns 1 even on success
                if (code === 0 || code === 1) {
                    resolve();
                }
                else {
                    reject(new Error(`explorer.exe exited with code ${code}`));
                }
            });
            proc.on('error', (err) => {
                proc.kill();
                reject(err);
            });
        });
    }
    const command = (0, node_os_1.platform)() === 'darwin' ? 'open' : 'xdg-open';
    const p = (0, node_child_process_1.spawn)(command, [(0, node_os_1.platform)() === 'darwin' ? '-R' : null, dirToOpen].filter(no_react_1.NoReactInternals.truthy));
    const stderrChunks = [];
    p.stderr.on('data', (d) => stderrChunks.push(d));
    return new Promise((resolve, reject) => {
        p.on('exit', (code) => {
            if (code === 0) {
                resolve();
            }
            else {
                const message = Buffer.concat(stderrChunks.map((buf) => Uint8Array.from(buf))).toString('utf8');
                reject(new Error(message));
            }
        });
        p.on('error', (err) => {
            p.kill();
            if (err) {
                reject(err);
            }
        });
    });
};
exports.openDirectoryInFinder = openDirectoryInFinder;
