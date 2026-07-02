"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.createSilentAudio = void 0;
const call_ffmpeg_1 = require("./call-ffmpeg");
const createSilentAudio = async ({ outName, indent, logLevel, binariesDirectory, cancelSignal, chunkLengthInSeconds, sampleRate, }) => {
    await (0, call_ffmpeg_1.callFf)({
        bin: 'ffmpeg',
        args: [
            '-f',
            'lavfi',
            '-i',
            `anullsrc=r=${sampleRate}`,
            '-c:a',
            'pcm_s16le',
            '-t',
            String(chunkLengthInSeconds),
            '-ar',
            String(sampleRate),
            outName,
        ],
        indent,
        logLevel,
        binariesDirectory,
        cancelSignal,
    });
};
exports.createSilentAudio = createSilentAudio;
