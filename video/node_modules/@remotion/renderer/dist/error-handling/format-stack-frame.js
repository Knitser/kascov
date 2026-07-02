"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.formatStackFrameLocationLine = exports.formatStackFrameCodeFrame = exports.makeStackFrameFileName = void 0;
const chalk_1 = require("../chalk");
const truthy_1 = require("../truthy");
const makeStackFrameFileName = (frame) => {
    return [
        frame.originalFileName,
        frame.originalLineNumber,
        frame.originalColumnNumber === 0 ? null : frame.originalColumnNumber,
    ]
        .filter(truthy_1.truthy)
        .join(':');
};
exports.makeStackFrameFileName = makeStackFrameFileName;
// Returns the rendered code frame body (line numbers + gutter), or null if the
// frame has no associated original script code.
const formatStackFrameCodeFrame = (frame) => {
    if (!frame.originalScriptCode || frame.originalScriptCode.length === 0) {
        return null;
    }
    const longestLineNumber = Math.max(...frame.originalScriptCode.map((script) => script.lineNumber)).toString().length;
    const alignLeftAmount = Math.min(...frame.originalScriptCode.map((c) => c.content.length - c.content.trimStart().length));
    return frame.originalScriptCode
        .map((c) => {
        const left = String(c.lineNumber).padStart(longestLineNumber, ' ');
        const right = c.content.substring(alignLeftAmount);
        if (c.highlight) {
            return `${left} │ ${right}`;
        }
        return `${chalk_1.chalk.gray(left)} │ ${chalk_1.chalk.gray(right)}`;
    })
        .join('\n');
};
exports.formatStackFrameCodeFrame = formatStackFrameCodeFrame;
// Returns a single gray "at functionName (fileName)" line, or null if there is
// no file location for the frame.
const formatStackFrameLocationLine = (frame) => {
    const fileName = (0, exports.makeStackFrameFileName)(frame);
    if (!fileName) {
        return null;
    }
    return chalk_1.chalk.gray(['at', frame.originalFunctionName, `${chalk_1.chalk.blueBright(`(${fileName})`)}`]
        .filter(truthy_1.truthy)
        .join(' '));
};
exports.formatStackFrameLocationLine = formatStackFrameLocationLine;
