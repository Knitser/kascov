import type { AnyZodSchema } from './zod-schema-type';
import type { JSONPath } from './zod-types';
import type { UpdaterFunction } from './ZodSwitch';
export declare const ZodTupleItemEditor: React.FC<{
    jsonPath: JSONPath;
    onChange: UpdaterFunction<unknown[]>;
    tupleItems: AnyZodSchema[];
    index: number;
    value: unknown;
    mayPad: boolean;
}>;
