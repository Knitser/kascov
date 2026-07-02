"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.usePixelDensity = exports.PixelDensityContext = void 0;
const react_1 = __importStar(require("react"));
const CanUseRemotionHooks_js_1 = require("./CanUseRemotionHooks.js");
exports.PixelDensityContext = react_1.default.createContext(null);
const getBrowserPixelDensity = () => {
    if (typeof window === 'undefined') {
        return 1;
    }
    return window.devicePixelRatio || 1;
};
/*
 * @description Retrieves the current pixel density. In previews, this corresponds to `window.devicePixelRatio`. During renders, this corresponds to the `scale` option.
 * @see [Documentation](https://www.remotion.dev/docs/use-pixel-density)
 */
const usePixelDensity = (options) => {
    const pixelDensity = (0, react_1.useContext)(exports.PixelDensityContext);
    const canUseRemotionHooks = (0, react_1.useContext)(CanUseRemotionHooks_js_1.CanUseRemotionHooks);
    if (pixelDensity !== null) {
        return pixelDensity;
    }
    if (canUseRemotionHooks || (options === null || options === void 0 ? void 0 : options.dontThrowIfOutsideOfRemotion)) {
        return getBrowserPixelDensity();
    }
    throw new Error([
        'usePixelDensity() was called outside of a Remotion context.',
        'This hook can only be called in a component that is being rendered by Remotion.',
        'If you want this hook to return the browser pixel density outside of Remotion, pass {dontThrowIfOutsideOfRemotion: true} as an option.',
        'If you think you called this hook in a Remotion component, make sure all versions of Remotion are aligned.',
    ].join('\n'));
};
exports.usePixelDensity = usePixelDensity;
