import type { InteractivitySchemaField } from '../interactivity-schema.js';
import type { EffectDefinition, EffectFactory } from './effect-types.js';
export declare const disabledEffectField: InteractivitySchemaField;
export declare const createEffect: <P, S>(definition: EffectDefinition<P, S>) => EffectFactory<P>;
