import type { ZodSafeParseResult } from './zod-schema-type';
import type { JSONPath } from './zod-types';
export declare const ZodFieldValidation: React.FC<{
    zodValidation: ZodSafeParseResult;
    path: JSONPath;
}>;
