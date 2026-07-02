import type { ZodSafeParseResult } from './zod-schema-type';
export type SchemaErrorMode = 'full' | 'compact';
export declare const ZodNotInstalled: React.FC<{
    readonly mode?: SchemaErrorMode;
}>;
export declare const NoSchemaDefined: React.FC<{
    readonly mode?: SchemaErrorMode;
}>;
export declare const NoDefaultProps: React.FC<{
    readonly mode?: SchemaErrorMode;
}>;
export declare const InvalidDefaultProps: React.FC<{
    zodValidationResult: ZodSafeParseResult;
}>;
export declare const InvalidSchema: React.FC<{
    zodValidationResult: ZodSafeParseResult;
}>;
export declare const TopLevelZodValue: React.FC<{
    readonly typeReceived: string;
    readonly mode?: SchemaErrorMode;
}>;
