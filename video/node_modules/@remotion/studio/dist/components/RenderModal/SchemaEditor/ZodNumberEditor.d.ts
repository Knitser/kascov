import React from 'react';
import { type AnyZodSchema } from './zod-schema-type';
import type { JSONPath } from './zod-types';
import type { UpdaterFunction } from './ZodSwitch';
export declare const ZodNumberEditor: React.FC<{
    readonly schema: AnyZodSchema;
    readonly jsonPath: JSONPath;
    readonly value: number;
    readonly setValue: UpdaterFunction<number>;
    readonly onRemove: null | (() => void);
    readonly mayPad: boolean;
}>;
