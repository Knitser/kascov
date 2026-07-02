export declare const maybeOpenBrowser: ({ browserArgs, browserFlag, shouldOpenBrowser, url, logLevel, }: {
    browserArgs: string;
    browserFlag: string;
    shouldOpenBrowser: boolean;
    url: string;
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
}) => Promise<{
    didOpenBrowser: boolean;
}>;
