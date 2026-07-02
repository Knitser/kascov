"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.wrapExecutableWithSetprivIfAvailable = exports.resolveSetprivPathOnLinux = void 0;
const node_child_process_1 = require("node:child_process");
let cachedSetprivPath;
/**
 * When the Node parent dies abruptly (SIGKILL, kernel OOM, etc.), child processes
 * are reparented to init and may keep running. On Linux, `setpriv(1)` uses
 * prctl(PR_SET_PDEATHSIG) so the child receives SIGKILL when its parent dies.
 *
 * @see https://github.com/remotion-dev/remotion/issues/7207
 */
const resolveSetprivPathOnLinux = () => {
    if (cachedSetprivPath !== undefined) {
        return cachedSetprivPath;
    }
    if (process.platform !== 'linux') {
        cachedSetprivPath = null;
        return null;
    }
    try {
        const path = (0, node_child_process_1.execSync)('command -v setpriv 2>/dev/null', {
            encoding: 'utf8',
            stdio: ['ignore', 'pipe', 'ignore'],
        }).trim();
        cachedSetprivPath = path ? path : null;
    }
    catch (_a) {
        cachedSetprivPath = null;
    }
    return cachedSetprivPath;
};
exports.resolveSetprivPathOnLinux = resolveSetprivPathOnLinux;
const wrapExecutableWithSetprivIfAvailable = ({ executablePath, args, }) => {
    const setpriv = (0, exports.resolveSetprivPathOnLinux)();
    if (!setpriv) {
        return { executablePath, args };
    }
    return {
        executablePath: setpriv,
        args: ['--pdeathsig', 'SIGKILL', executablePath, ...args],
    };
};
exports.wrapExecutableWithSetprivIfAvailable = wrapExecutableWithSetprivIfAvailable;
