"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.initialAggregateRenderProgress = void 0;
const initialAggregateRenderProgress = () => ({
    rendering: null,
    browser: {
        progress: 0,
        alreadyAvailable: true,
        doneIn: null,
    },
    downloads: [],
    stitching: null,
    bundling: null,
    copyingState: {
        bytes: 0,
        doneIn: null,
    },
    artifactState: {
        received: [],
    },
    logs: [],
});
exports.initialAggregateRenderProgress = initialAggregateRenderProgress;
