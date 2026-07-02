import type { UpdateAvailableResponse } from '@remotion/studio-shared';
export declare const getRemotionVersion: () => any;
export declare const isUpdateAvailableWithTimeout: (remotionRoot: string, logLevel: "error" | "info" | "trace" | "verbose" | "warn") => Promise<UpdateAvailableResponse>;
