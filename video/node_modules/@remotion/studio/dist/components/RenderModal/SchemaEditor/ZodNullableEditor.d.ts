import type { AnyZodSchema } from './zod-schema-type';
import type { JSONPath } from './zod-types';
import type { UpdaterFunction } from './ZodSwitch';
export declare const ZodNullableEditor: React.FC<{
    jsonPath: JSONPath;
    value: unknown;
    schema: AnyZodSchema;
    setValue: UpdaterFunction<unknown>;
    onRemove: null | (() => void);
    mayPad: boolean;
}>;
