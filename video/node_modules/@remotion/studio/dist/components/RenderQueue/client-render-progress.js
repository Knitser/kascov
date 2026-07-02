"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getClientRenderProgressMessage = exports.formatEtaString = void 0;
const formatEtaString = (timeRemainingInMilliseconds) => {
    const remainingTime = timeRemainingInMilliseconds / 1000;
    const remainingTimeHours = Math.floor(remainingTime / 3600);
    const remainingTimeMinutes = Math.floor((remainingTime % 3600) / 60);
    const remainingTimeSeconds = Math.floor(remainingTime % 60);
    return [
        remainingTimeHours ? `${remainingTimeHours}h` : null,
        remainingTimeMinutes ? `${remainingTimeMinutes}m` : null,
        `${remainingTimeSeconds}s`,
    ]
        .filter((value) => Boolean(value))
        .join(' ');
};
exports.formatEtaString = formatEtaString;
const getClientRenderProgressMessage = (progress) => {
    if (progress.totalFrames === 0) {
        return 'Getting composition';
    }
    if (progress.doneIn !== null) {
        return `Encoded ${progress.totalFrames}/${progress.totalFrames}`;
    }
    if (progress.renderEstimatedTime > 0) {
        const etaString = `, time remaining: ${(0, exports.formatEtaString)(progress.renderEstimatedTime)}`;
        return `Rendering ${progress.encodedFrames}/${progress.totalFrames}${etaString}`;
    }
    return `Encoded ${progress.encodedFrames}/${progress.totalFrames}`;
};
exports.getClientRenderProgressMessage = getClientRenderProgressMessage;
