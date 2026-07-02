import React from 'react';
import type { AnyZodSchema } from './zod-schema-type';
import type { JSONPath } from './zod-types';
export type UpdaterFunction<T> = (updater: (oldValue: T) => T, { shouldSave }: {
    shouldSave: boolean;
}) => void;
export declare const ZodSwitch: React.FC<{
    readonly schema: AnyZodSchema;
    readonly jsonPath: JSONPath;
    readonly value: unknown;
    readonly setValue: UpdaterFunction<unknown>;
    readonly onRemove: null | (() => void);
    readonly mayPad: boolean;
}>;
