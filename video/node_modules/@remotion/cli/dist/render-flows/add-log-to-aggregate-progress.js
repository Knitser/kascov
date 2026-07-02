"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.addLogToAggregateProgress = void 0;
const renderer_1 = require("@remotion/renderer");
const addLogToAggregateProgress = ({ logs, logLogLevel, logLevel, previewString, tag, }) => {
    if (renderer_1.RenderInternals.isEqualOrBelowLogLevel(logLevel, logLogLevel)) {
        logs.push({ logLevel: logLogLevel, previewString, tag });
        if (logs.length > 3) {
            logs.shift();
        }
    }
};
exports.addLogToAggregateProgress = addLogToAggregateProgress;
