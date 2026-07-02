import React from 'react';
import { type SchemaErrorMode } from './SchemaErrorMessages';
import type { AnyZodSchema } from './zod-schema-type';
import type { UpdaterFunction } from './ZodSwitch';
export declare const SchemaEditor: React.FC<{
    readonly schema: AnyZodSchema;
    readonly value: Record<string, unknown>;
    readonly setValue: UpdaterFunction<Record<string, unknown>>;
    readonly scrollableContainer?: boolean;
    readonly contentInset?: number;
    readonly errorMode?: SchemaErrorMode;
}>;
