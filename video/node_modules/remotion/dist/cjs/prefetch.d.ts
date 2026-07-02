export declare const removeAndGetHashFragment: (src: string) => number | null;
export declare const getSrcWithoutHash: (src: string) => string;
export declare const usePreload: (src: string) => string;
type FetchAndPreload = {
    free: () => void;
    waitUntilDone: () => Promise<string>;
};
export type PrefetchOnProgress = (options: {
    totalBytes: number | null;
    loadedBytes: number;
}) => void;
export declare const prefetch: (src: string, options?: {
    method?: "base64" | "blob-url" | undefined;
    contentType?: string | undefined;
    onProgress?: PrefetchOnProgress | undefined;
    credentials?: RequestCredentials | undefined;
    logLevel?: "error" | "info" | "trace" | "verbose" | "warn" | undefined;
} | undefined) => FetchAndPreload;
export {};
