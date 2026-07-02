"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.detectRemotionServer = void 0;
const http_1 = __importDefault(require("http"));
const detectRemotionServer = ({ port, cwd, hostname, }) => {
    return new Promise((resolve) => {
        const req = http_1.default.get({
            hostname,
            port,
            path: '/__remotion_config',
            timeout: 1000,
        }, (res) => {
            if (res.statusCode !== 200) {
                res.resume();
                return resolve({ type: 'not-remotion' });
            }
            let data = '';
            res.on('data', (chunk) => {
                data += chunk;
            });
            res.on('error', () => {
                resolve({ type: 'not-remotion' });
            });
            res.on('end', () => {
                try {
                    const json = JSON.parse(data);
                    if (json.isRemotion !== true) {
                        return resolve({ type: 'not-remotion' });
                    }
                    // Normalize paths for comparison to avoid issues with different separators or casing on Windows
                    const normalize = (p) => p.replace(/\\/g, '/').toLowerCase();
                    if (normalize(json.cwd) === normalize(cwd)) {
                        return resolve({ type: 'match' });
                    }
                    return resolve({ type: 'mismatch' });
                }
                catch (_a) {
                    resolve({ type: 'not-remotion' });
                }
            });
        });
        req.on('error', () => resolve({ type: 'not-remotion' }));
        req.on('timeout', () => {
            req.destroy();
        });
    });
};
exports.detectRemotionServer = detectRemotionServer;
