"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.maybeOpenBrowser = void 0;
const renderer_1 = require("@remotion/renderer");
const better_opn_1 = require("./better-opn");
const maybeOpenBrowser = async ({ browserArgs, browserFlag, shouldOpenBrowser, url, logLevel, }) => {
    var _a;
    if (((_a = process.env.BROWSER) !== null && _a !== void 0 ? _a : '').toLowerCase() === 'none') {
        renderer_1.RenderInternals.Log.verbose({ indent: false, logLevel }, 'Not opening browser, reason: env BROWSER=none was set');
        return { didOpenBrowser: false };
    }
    if (shouldOpenBrowser) {
        await (0, better_opn_1.openBrowser)({
            url,
            browserArgs,
            browserFlag,
        });
    }
    else {
        renderer_1.RenderInternals.Log.verbose({ indent: false, logLevel }, 'Not opening browser, reason: --no-open specified or config file');
    }
    return { didOpenBrowser: shouldOpenBrowser };
};
exports.maybeOpenBrowser = maybeOpenBrowser;
