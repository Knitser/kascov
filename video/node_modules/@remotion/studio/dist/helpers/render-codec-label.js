"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.renderHumanReadableVideoCodec = exports.renderHumanReadableAudioCodec = void 0;
const audioCodecLabels = {
    aac: 'AAC',
    ac3: 'AC3',
    alaw: 'A-law',
    eac3: 'E-AC3',
    flac: 'FLAC',
    mp3: 'MP3',
    opus: 'Opus',
    'pcm-f32': 'PCM 32-bit float',
    'pcm-f32be': 'PCM 32-bit big-endian float',
    'pcm-f64': 'PCM 64-bit little-endian float',
    'pcm-f64be': 'PCM 64-bit big-endian float',
    'pcm-s16': 'PCM 16-bit signed integer',
    'pcm-s16be': 'PCM 16-bit big-endian signed integer',
    'pcm-s24': 'PCM 24-bit signed integer',
    'pcm-s24be': 'PCM 24-bit big-endian signed integer',
    'pcm-s32': 'PCM 32-bit signed integer',
    'pcm-s32be': 'PCM 32-bit big-endian signed integer',
    'pcm-s8': 'PCM 8-bit signed integer',
    'pcm-u8': 'PCM 8-bit unsigned integer',
    ulaw: 'u-law',
    vorbis: 'Vorbis',
};
const renderHumanReadableAudioCodec = (codec) => {
    var _a;
    if (codec === null) {
        return 'Unknown';
    }
    return (_a = audioCodecLabels[codec]) !== null && _a !== void 0 ? _a : codec;
};
exports.renderHumanReadableAudioCodec = renderHumanReadableAudioCodec;
const videoCodecLabels = {
    av1: 'AV1',
    avc: 'H.264',
    hevc: 'H.265',
    vp8: 'VP8',
    vp9: 'VP9',
};
const renderHumanReadableVideoCodec = (codec) => {
    var _a;
    if (codec === null) {
        return 'Unknown';
    }
    return (_a = videoCodecLabels[codec]) !== null && _a !== void 0 ? _a : codec;
};
exports.renderHumanReadableVideoCodec = renderHumanReadableVideoCodec;
