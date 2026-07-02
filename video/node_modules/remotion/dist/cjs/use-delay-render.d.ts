import type { cancelRender as cancelRenderOriginal } from './cancel-render.js';
import type { DelayRenderOptions, DelayRenderScope } from './delay-render.js';
type DelayRenderFn = (label?: string, options?: DelayRenderOptions) => number;
type ContinueRenderFn = (handle: number) => void;
export declare const DelayRenderContextType: import("react").Context<DelayRenderScope | null>;
export declare const useDelayRender: () => {
    delayRender: DelayRenderFn;
    continueRender: ContinueRenderFn;
    cancelRender: typeof cancelRenderOriginal;
};
export {};
