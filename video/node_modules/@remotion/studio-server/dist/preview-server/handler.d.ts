import type { IncomingMessage, ServerResponse } from 'node:http';
import type { ApiHandler, QueueMethods } from './api-types';
export declare const handleRequest: <Req, Res>({ remotionRoot, request, response, entryPoint, handler, logLevel, methods, binariesDirectory, publicDir, }: {
    remotionRoot: string;
    publicDir: string;
    request: IncomingMessage;
    response: ServerResponse<IncomingMessage>;
    entryPoint: string;
    binariesDirectory: string | null;
    handler: ApiHandler<Req, Res>;
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
    methods: QueueMethods;
}) => Promise<void>;
