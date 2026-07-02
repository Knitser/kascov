import { type AnyZodSchema } from './zod-schema-type';
import type { JSONPath } from './zod-types';
import type { UpdaterFunction } from './ZodSwitch';
export declare const ZodOrNullishEditor: React.FC<{
    jsonPath: JSONPath;
    value: unknown;
    schema: AnyZodSchema;
    innerSchema: AnyZodSchema;
    setValue: UpdaterFunction<unknown>;
    onRemove: null | (() => void);
    nullishValue: null | undefined;
    mayPad: boolean;
}>;
