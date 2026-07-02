"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getCompositionWithDimensionOverride = void 0;
const get_composition_id_1 = require("./get-composition-id");
const getCompositionWithDimensionOverride = async ({ height, width, fps, durationInFrames, args, compositionIdFromUi, chromiumOptions, envVariables, port, puppeteerInstance, timeoutInMilliseconds, browserExecutable, serveUrlOrWebpackUrl, indent, serializedInputPropsWithCustomSchema, logLevel, server, offthreadVideoCacheSizeInBytes, offthreadVideoThreads, binariesDirectory, onBrowserDownload, chromeMode, mediaCacheSizeInBytes, }) => {
    const returnValue = await (0, get_composition_id_1.getCompositionId)({
        args,
        compositionIdFromUi,
        indent,
        serveUrlOrWebpackUrl,
        logLevel,
        browserExecutable,
        chromiumOptions,
        envVariables,
        serializedInputPropsWithCustomSchema,
        port,
        puppeteerInstance,
        timeoutInMilliseconds,
        server,
        offthreadVideoCacheSizeInBytes,
        binariesDirectory,
        onBrowserDownload,
        chromeMode,
        offthreadVideoThreads,
        mediaCacheSizeInBytes,
    });
    return {
        ...returnValue,
        config: {
            ...returnValue.config,
            height: height !== null && height !== void 0 ? height : returnValue.config.height,
            width: width !== null && width !== void 0 ? width : returnValue.config.width,
            fps: fps !== null && fps !== void 0 ? fps : returnValue.config.fps,
            durationInFrames: durationInFrames !== null && durationInFrames !== void 0 ? durationInFrames : returnValue.config.durationInFrames,
        },
    };
};
exports.getCompositionWithDimensionOverride = getCompositionWithDimensionOverride;
