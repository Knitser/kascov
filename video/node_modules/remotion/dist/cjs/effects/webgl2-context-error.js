"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.createWebGL2ContextError = exports.createWebGLContextError = exports.WEBGL2_CONTEXT_ERROR_MARKER = void 0;
const WEBGL_CONTEXT_DOCS_URL = 'https://remotion.dev/docs/troubleshooting/webgl2-context';
exports.WEBGL2_CONTEXT_ERROR_MARKER = 'Failed to acquire WebGL2 context';
const webGlContextErrorMessage = (versionLabel, effectName) => `Failed to acquire ${versionLabel} context for ${effectName}. ` +
    'Pass --gl=angle when using the CLI, set chromiumOptions: { gl: "angle" } when using SSR APIs, ' +
    'or set "OpenGL render backend" to "angle" in the Advanced section when rendering in the Studio. ' +
    `See ${WEBGL_CONTEXT_DOCS_URL}`;
const createWebGLContextError = (effectName) => new Error(webGlContextErrorMessage('WebGL', effectName));
exports.createWebGLContextError = createWebGLContextError;
const createWebGL2ContextError = (effectName) => new Error(webGlContextErrorMessage('WebGL2', effectName));
exports.createWebGL2ContextError = createWebGL2ContextError;
