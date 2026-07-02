import React from 'react';
import { type AnyZodSchema } from './zod-schema-type';
import type { JSONPath } from './zod-types';
import type { UpdaterFunction } from './ZodSwitch';
export type ObjectDiscrimatedUnionReplacement = {
    discriminator: string;
    markup: React.ReactNode;
};
export declare const ZodObjectEditor: React.FC<{
    readonly schema: AnyZodSchema;
    readonly jsonPath: JSONPath;
    readonly value: Record<string, unknown>;
    readonly setValue: UpdaterFunction<Record<string, unknown>>;
    readonly onRemove: null | (() => void);
    readonly mayPad: boolean;
    readonly discriminatedUnionReplacement: ObjectDiscrimatedUnionReplacement | null;
}>;
