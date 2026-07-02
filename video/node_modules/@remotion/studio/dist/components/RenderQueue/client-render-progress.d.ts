import type { ClientRenderJobProgress } from './client-side-render-types';
export declare const formatEtaString: (timeRemainingInMilliseconds: number) => string;
export declare const getClientRenderProgressMessage: (progress: ClientRenderJobProgress) => string;
