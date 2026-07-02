import type { InteractivitySchema } from '../interactivity-schema.js';
export type Backend = '2d' | 'webgl2' | 'webgpu';
type AnyGpuDevice = unknown;
export type EffectApplyParams<P, S> = {
    readonly source: CanvasImageSource;
    readonly target: HTMLCanvasElement;
    readonly state: S;
    readonly params: P;
    readonly width: number;
    readonly height: number;
    readonly gpuDevice: AnyGpuDevice | null;
    /**
     * When `true`, WebGL `texImage2D` uploads use `UNPACK_FLIP_Y_WEBGL` so DOM-style
     * canvas sources match clip-space UVs. Set by `runEffectChain` — `false` for
     * `ImageBitmap` bridges from WebGL, which are already oriented for upload.
     */
    readonly flipSourceY: boolean;
};
export type EffectDefinition<P, S = unknown> = {
    readonly type: string;
    readonly label: string;
    readonly documentationLink: string | null;
    readonly backend: Backend;
    /**
     * Stable string for comparing effect instances: two descriptors with the same
     * `definition` and the same `calculateKey(params)` are treated as equivalent
     * for memoization (e.g. timeline registration) even when `params` is a new object
     * reference each render.
     */
    readonly calculateKey: (params: P) => string;
    readonly setup: (target: HTMLCanvasElement) => S;
    readonly apply: (params: EffectApplyParams<P, S>) => void;
    readonly cleanup: (state: S) => void;
    readonly schema: InteractivitySchema;
    /** Throws when mandatory params are missing or invalid. Called by `createEffect` before returning a descriptor. */
    readonly validateParams: (params: P) => void;
};
type BaseEffectDescriptor<P = unknown> = {
    readonly definition: EffectDefinition<P, unknown>;
    readonly effectKey: string;
    readonly params: P;
};
export type EffectDescriptor<P = unknown> = BaseEffectDescriptor<P> & {
    readonly memoized: false;
};
export type EffectDefinitionAndStack<P = unknown> = BaseEffectDescriptor<P> & {
    readonly memoized: true;
};
export type EffectsProp = ReadonlyArray<EffectDescriptor<unknown>>;
export type EffectFactory<P> = {} extends P ? (params?: P & {
    readonly disabled?: boolean;
}) => EffectDescriptor<unknown> : (params: P & {
    readonly disabled?: boolean;
}) => EffectDescriptor<unknown>;
export {};
