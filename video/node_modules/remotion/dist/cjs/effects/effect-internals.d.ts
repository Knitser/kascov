import type { Backend, EffectDefinitionAndStack } from './effect-types.js';
export type Run = {
    readonly backend: Backend;
    readonly effects: EffectDefinitionAndStack<unknown>[];
};
export declare const groupByBackend: (effects: EffectDefinitionAndStack<unknown>[]) => Run[];
