import type { BrowserExecutable, ChromiumOptions, HeadlessBrowser, OnBrowserDownload, RemotionServer } from '@remotion/renderer';
import type { VideoConfig } from 'remotion';
export declare const getCompositionId: ({ args, compositionIdFromUi, serializedInputPropsWithCustomSchema, puppeteerInstance, envVariables, timeoutInMilliseconds, chromiumOptions, port, browserExecutable, serveUrlOrWebpackUrl, logLevel, indent, server, offthreadVideoCacheSizeInBytes, offthreadVideoThreads, binariesDirectory, onBrowserDownload, chromeMode, mediaCacheSizeInBytes, }: {
    args: (string | number)[];
    compositionIdFromUi: string | null;
    serializedInputPropsWithCustomSchema: string;
    puppeteerInstance: HeadlessBrowser | undefined;
    envVariables: Record<string, string>;
    timeoutInMilliseconds: number;
    chromiumOptions: ChromiumOptions;
    port: number | null;
    browserExecutable: BrowserExecutable;
    serveUrlOrWebpackUrl: string;
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
    indent: boolean;
    server: RemotionServer;
    offthreadVideoCacheSizeInBytes: number | null;
    offthreadVideoThreads: number | null;
    binariesDirectory: string | null;
    onBrowserDownload: OnBrowserDownload;
    chromeMode: "chrome-for-testing" | "headless-shell";
    mediaCacheSizeInBytes: number | null;
}) => Promise<{
    compositionId: string;
    reason: string;
    config: VideoConfig;
    argsAfterComposition: (string | number)[];
}>;
