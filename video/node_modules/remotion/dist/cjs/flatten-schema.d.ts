import type { InteractivitySchema } from './interactivity-schema';
export type ResolveValue = (key: string) => unknown;
export declare const flattenActiveSchema: (schema: InteractivitySchema, resolve: ResolveValue) => InteractivitySchema;
export declare const getFlatSchemaWithAllKeys: (schema: InteractivitySchema) => InteractivitySchema;
