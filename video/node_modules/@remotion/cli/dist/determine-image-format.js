"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.determineFinalStillImageFormat = void 0;
const client_1 = require("@remotion/renderer/client");
const { cliFlag } = client_1.BrowserSafeApis.options.stillImageFormatOption;
const deriveExtensionFromFilename = (filename) => {
    if (filename === null || filename === void 0 ? void 0 : filename.endsWith('.png')) {
        return 'png';
    }
    if (filename === null || filename === void 0 ? void 0 : filename.endsWith('.jpg')) {
        return 'jpeg';
    }
    if (filename === null || filename === void 0 ? void 0 : filename.endsWith('.jpeg')) {
        return 'jpeg';
    }
    if (filename === null || filename === void 0 ? void 0 : filename.endsWith('.pdf')) {
        return 'pdf';
    }
    if (filename === null || filename === void 0 ? void 0 : filename.endsWith('.webp')) {
        return 'webp';
    }
    return null;
};
const determineFinalStillImageFormat = ({ downloadName, outName, configuredImageFormat, isLambda, fromUi, }) => {
    if (fromUi) {
        return { format: fromUi, source: 'via UI' };
    }
    const outNameExtension = deriveExtensionFromFilename(outName);
    const downloadNameExtension = deriveExtensionFromFilename(downloadName);
    const outNameDescription = isLambda ? 'S3 output key' : 'out name';
    if (outNameExtension &&
        downloadNameExtension &&
        outNameExtension !== downloadNameExtension) {
        throw new TypeError(`Image format mismatch: ${outName} was given as the ${outNameDescription} and ${downloadName} was given as the download name, but the extensions don't match.`);
    }
    if (downloadNameExtension) {
        if (configuredImageFormat &&
            downloadNameExtension !== configuredImageFormat) {
            throw new TypeError(`Image format mismatch: ${downloadName} was given as the download name, but the image format "${configuredImageFormat}" was configured via --${cliFlag} or Config.setStillImageFormat(). The image formats must match.`);
        }
        return { format: downloadNameExtension, source: 'Download name extension' };
    }
    if (outNameExtension) {
        if (configuredImageFormat && outNameExtension !== configuredImageFormat) {
            throw new TypeError(`Image format mismatch: ${outName} was given as the ${outNameDescription}, but the image format "${configuredImageFormat}" was configured via --${cliFlag} or Config.setStillImageFormat(). The image formats must match.`);
        }
        return { format: outNameExtension, source: 'Out name extension' };
    }
    if (configuredImageFormat !== null) {
        return { format: configuredImageFormat, source: `--${cliFlag} or config` };
    }
    return { format: 'png', source: 'Default' };
};
exports.determineFinalStillImageFormat = determineFinalStillImageFormat;
